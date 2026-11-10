# Jiro-ikusu's code (Training, README §8.1), which jiro.py runs once it holds the core's buffers. Shipped
# precompiled as jiro_app.mpy (wedgie/scripts/build-app.sh). The core is the referee: each tick's
# inputs go into the README §6.1 mask and through the core's Recorder, and the app only draws
# what the core reports. wedgie/scripts/qemu_app_check.py plays it on MicroPython's qemu port.
#
# The numbers are const(): mpy-cross writes each where it is used, so a name costs the wedgie no
# RAM and the tick no lookup.
import gc
import os
import struct
import time
from array import array
from binascii import hexlify

from micropython import const

import jiro_core as jc
import lcd
import save
import ui

# The level file, which wedgie/scripts/make_app_levels.py writes: MAGIC, the rules version (u16)
# and the level count (u8), then per level its name behind a length byte, its weightBp (u16), its
# level id and its level bytes.
MAGIC = b"JL01"
_HEADER_BYTES = const(7)
_LEVEL_ID_BYTES = const(32)
_LEVEL_BYTES = const(472)

# The core's calls. A journal's header holds the level id after its version, rules version, mode
# and run nonce (README §6.3).
_TRAINING = const(0)
_TRAINING_INDEX = const(0)  # Training plays at levelIndex 0, as in the browser
_JOURNAL_LEVEL_ID = const(36)
_STEP_ERROR = const(0x100)  # step returns an error code above it, an end reason below

# The fields of jc.status (jiro_status in natmod/rust/src/lib.rs), in its order.
_END = const(0)
_TICKS = const(1)
_TIME_LIMIT = const(2)
_LIVES = const(3)
_SHIELD = const(4)
_INVULNERABLE = const(5)
_CUTTING = const(6)
_CLAIM = const(7)
_TARGET = const(8)
_PLAYER_X = const(9)
_PLAYER_Y = const(10)
_BOSS_X = const(11)
_BOSS_Y = const(12)
_BOSS_SIZE = const(13)
_MINIONS = const(14)  # from here, each of the 3 minion slots' state, then its x and y
_MINION_FIELDS = const(3)
_STATUS_LEN = const(23)
_LIVE = const(1)  # a minion slot's state

# The end reasons (README §4.10), which step returns and status reports.
END_TITLES = ("", "Committed", "Cleared", "Out of lives", "Time up")
_CLEARED = const(2)

# Tick k is due at t0 + floor(k × 10^6 / 15) µs: 66,666 µs and two thirds, on an absolute schedule,
# which starts again from a tick that is more than one tick late (wait_until).
_TICK_US = const(66666)
_OVERLAY_MS = const(1500)
_SUMMARY_GUARD_MS = const(500)
_MENU_POLL_MS = const(30)
# The app collects every _GC_EVERY_TICKS ticks, after a tick whose work took under _GC_LIGHT_US, so
# that the garbage of a tick's drawing (the frames and lists of the firmware's show_rect) is not
# collected inside a heavy tick: a collection took up to 27.5 ms in the app on a wedgie, and a
# light tick and a collection fit in a tick (round E1: no overrun, against three before).
_GC_EVERY_TICKS = const(8)
_GC_LIGHT_US = const(25000)
_PRINT_STALL_MS = const(100)  # a print that takes longer finds the USB port open but not read
_JOURNAL_LINE_BYTES = const(48)  # of the journal, in hex, per line printed

# The controls of README §2 (D8) and their bits of the README §6.1 mask. Y and the joystick press
# are not controls, so they never reach the mask.
CONTROL_BITS = {"up": 1, "down": 2, "left": 4, "right": 8, "A": 16, "B": 32, "X": 64}

# The screen: 240 × 240 pixels, a cell of the 60 × 60 field on 4 × 4 of them.
_CELL_PX = const(4)
_FIELD_PX = const(240)
# The colors of the cell states EMPTY, CLAIMED, TRAIL and BOSS_ZONE (README §4.1) and of the
# entities: the browser's (web/src/render.ts), each the nearest of the screen's 16 (lcd.color),
# but the boss's. Its magenta, #ff3cf0, comes out as the trail's red, so the boss is the palette's
# yellow, which nothing else on the field is.
CELL_COLORS = bytes((lcd.color(0, 0, 0), lcd.color(0x28, 0x50, 0xFF), lcd.color(0xFF, 0x2A, 0x2A), lcd.color(0, 0, 0)))
BOSS = lcd.color(255, 220, 0)
MINION = lcd.color(0x3C, 0xFF, 0x78)
PLAYER = lcd.color(0xFF, 0xFF, 0xFF)
PLAYER_PROTECTED = lcd.color(0x9A, 0x9A, 0x9A)
# A cell is 4 pixels wide, as thin as the trail it cuts: the player is drawn 1 pixel larger on
# each side, in a black ring 1 pixel wider, so it stands out on the trail, the claimed area and
# the borders. The ring reaches into the neighbouring cells, which the next tick restores.
PLAYER_RING = lcd.color(0, 0, 0)
_PLAYER_MARGIN_PX = const(2)
_RING_PX = const(_CELL_PX + 2 * _PLAYER_MARGIN_PX)
# A changed area at least this wide is pushed as full-width rows, the screen's fast path: a row of
# show_rect costs about 45 µs plus 0.26 µs a pixel, a row of a full-width band about 75 µs, which
# also allocates nothing per chunk of rows (wedgie.dev/code.md; the firmware's lcd.py).
_BAND_MIN_PX = const(128)


def main(stack_slot, recorder, shadow, level, level_file, bests, heap):
    """Plays Training in jiro.py's buffers. `stack_slot` is a list that holds the core's stack, so
    that the app can lend the stack's block to save.store; `heap` measures the heap at a stage."""
    if jc.recorder_size() > len(recorder) or len(level) != _LEVEL_BYTES:
        raise ValueError("jiro_core needs a %d B Recorder and a %d B level buffer" % (jc.recorder_size(), _LEVEL_BYTES))
    if not isinstance(bests, dict):
        raise ValueError("the bests save is not a dict")
    Training(stack_slot, recorder, shadow, level, level_file, bests, heap).run()


def read_levels(f):
    """The level file's index: (name, weightBp, level id, offset of the level bytes) per level."""
    levels = []
    f.seek(0)
    if f.read(4) != MAGIC:
        raise ValueError("the level file is not a JL01 level file")
    rules_version, count = struct.unpack(">HB", f.read(3))
    if rules_version != jc.rules_version():
        raise ValueError("the levels are scored for rules version %d, the core runs %d" % (rules_version, jc.rules_version()))
    offset = _HEADER_BYTES
    for _ in range(count):
        name_bytes = f.read(1)[0]
        name = f.read(name_bytes).decode()
        weight = struct.unpack(">H", f.read(2))[0]
        level_id = f.read(_LEVEL_ID_BYTES)
        offset += 1 + name_bytes + 2 + _LEVEL_ID_BYTES
        levels.append((name, weight, level_id, offset))
        f.seek(_LEVEL_BYTES, 1)
        offset += _LEVEL_BYTES
    return levels


def sample(keys):
    """The tick's input mask: the controls held now or pressed since the last sample, so a tap
    shorter than a tick counts once. Held keys are read first, then the presses."""
    mask = 0
    for name in CONTROL_BITS:
        if keys.held(name):
            mask |= CONTROL_BITS[name]
    for name in keys.pressed():
        if name in CONTROL_BITS:
            mask |= CONTROL_BITS[name]
    return mask


def wait_until(due):
    """Waits for a tick's due time and returns it. A tick up to one tick late runs at once and
    keeps its due time, so the next comes sooner and the level keeps its 15 ticks a second. A
    tick later than that returns now as its due time: the schedule starts again from it, and
    the level does not run the ticks it is behind by one after the other (a wedgie that slept on
    its battery in the middle of a level would otherwise play the rest of it at once). A due
    time is never more than one tick ahead, so one that ticks_diff reads further ahead is more
    than 2^29 µs late, past its range: that tick is late too, and never slept for."""
    left = time.ticks_diff(due, time.ticks_us())
    if left > _TICK_US + 1 or left < -_TICK_US:
        return time.ticks_us()
    if left > 2000:
        time.sleep_ms(left // 1000 - 1)  # the firmware answers USB in here
    while time.ticks_diff(due, time.ticks_us()) > 0:
        pass
    return due


def percent(bp):
    """Basis points as the screens show them, to the hundredth of a percent: 677 is 6.77 %."""
    return "%d.%02d %%" % (bp // 100, bp % 100)


def lives(count):
    return "1 life" if count == 1 else "%d lives" % count


def store(bests):
    """Stores the bests: None, or the errno of the OSError save.store raised, 0 for its "flash
    full", which has none. A call of its own, so that no reference to the exception outlives it:
    the garbage collector scans the stack conservatively, and the frame that catches an exception
    keeps pointers to it."""
    try:
        save.store("bests", bests)
    except OSError as error:
        return error.errno if isinstance(error.errno, int) else 0


class Training:
    """The 16 levels from the level select to each level's summary, in the buffers jiro.py made.
    From its start on it allocates nothing sizable: a level start reads into the level buffer, a
    tick allocates nothing on the heap but its drawing's frame, and a screen's strings are small."""

    def __init__(self, stack_slot, recorder, shadow, level, level_file, bests, heap):
        self.stack_slot, self.recorder, self.shadow = stack_slot, recorder, shadow
        self.level, self.level_file, self.bests = level, level_file, bests
        self.heap = heap
        self.unsaved = None  # while a store has failed and the save lacks a best: that store's errno
        self.screen, self.keys = lcd.LCD(), lcd.Keys()
        heap("lcd")
        self.frame = self.screen.buffer
        # The screen's calls of a tick, bound once: a call through the screen would bind one each time.
        self.show_rect, self.fill_rect, self.show_band = self.screen.show_rect, self.screen.fill_rect, self.screen.show
        # What the core reports after the last render and after the one before, where the entities
        # still stand on the screen; and the rectangle of the cells a paint changed.
        self.status, self.previous = array("H", [0] * _STATUS_LEN), array("H", [0] * _STATUS_LEN)
        self.rect = array("H", [0] * 4)
        self.levels = read_levels(level_file)
        # Room in the bests for a key per level, made while the heap has room: a new best then
        # never makes the dict grow, into a block the size of its table, at the end of a level,
        # when the heap may have none. MicroPython's dict keeps its table when keys are deleted.
        for key in range(len(self.levels)):
            bests[key] = 0
        for key in range(len(self.levels)):
            del bests[key]
        heap("app")

    # The screens around a level.

    def run(self):
        index = 0
        while True:
            index = self.choose(index)
            if self.play(index) == _CLEARED:
                index += 1
                if index == len(self.levels):
                    self.congrats()
                    index = 0

    def choose(self, index):
        """The level select, outside play and never journaled: the joystick picks, green plays."""
        count = len(self.levels)
        self.heap("sel")
        while True:
            best = self.bests.get(self.best_key(index))
            ui.page(self.screen, "Level %d of %d" % (index + 1, count),
                    [(self.levels[index][0], ui.INK),
                     ("no best yet" if best is None else "best %d" % best, ui.MUTED)],
                    "< > {g} play")
            step = 0
            while not step:
                pressed = self.keys.pressed()
                if "A" in pressed:
                    return index
                step = ("right" in pressed or "down" in pressed) - ("left" in pressed or "up" in pressed)
                time.sleep_ms(_MENU_POLL_MS)
            index = (index + step) % count

    def congrats(self):
        """After the last level's clear, instead of its level select again: the game is won."""
        ui.page(self.screen, "Congrats!", [("visit jiro-ikusu.win", ui.INK)], "{g} level 1")
        self.wait_for_green()

    def wait_for_green(self):
        """Presses made before a screen's first 0.5 s do not skip it."""
        time.sleep_ms(_SUMMARY_GUARD_MS)
        self.keys.pressed()
        while "A" not in self.keys.pressed():
            time.sleep_ms(_MENU_POLL_MS)

    # A level.

    def play(self, index):
        """Plays the level from its start to the green press on its summary; returns how it ended."""
        self.start(index)
        self.heap("lvl")
        self.redraw()
        self.overlay(index)
        self.redraw()
        self.keys.pressed()  # presses made before tick 0 are not inputs
        gc.collect()
        end, timing = self.ticks()
        self.finish(index, end, timing)
        return end

    def start(self, index):
        """Reads the level into its buffer and starts it in the core, under a fresh run nonce."""
        name, _, level_id, offset = self.levels[index]
        self.level_file.seek(offset)
        if self.level_file.readinto(self.level) != _LEVEL_BYTES:
            raise ValueError("the level file ends inside level %s" % name)
        # Not the firmware's wedgie.rand: its first call probes the I2C bus for a secure element and
        # keeps the drivers it imports, about 13 KB of a wedgie's heap (round E1).
        code = jc.start(self.recorder, self.level, _TRAINING, os.urandom(32), _TRAINING_INDEX, self.stack_slot[0])
        if code:
            raise RuntimeError("the core rejects level %s: code %d" % (name, code))
        if jc.journal(self.recorder, _JOURNAL_LEVEL_ID, _JOURNAL_LEVEL_ID + _LEVEL_ID_BYTES) != level_id:
            raise ValueError("level %s is not the calibration's" % name)

    def overlay(self, index):
        """The brief overlay at level start, before tick 0: the level, its target and the lives."""
        s, status = self.screen, self.status
        target = status[_TARGET]  # a whole percentage on most levels, 76.50 % or 81.50 % on two
        s.fill_rect(24, 80, 192, 80, ui.WHITE)
        s.center_text(self.levels[index][0], 92, ui.INK, 2)
        s.center_text("target " + (percent(target) if target % 100 else "%d %%" % (target // 100)), 122, ui.MUTED, 1)
        s.center_text(lives(status[_LIVES]), 138, ui.MUTED, 1)
        s.show()
        time.sleep_ms(_OVERLAY_MS)

    def ticks(self):
        """Steps the level once per tick until it ends, on wait_until's schedule: a late tick is
        late, never skipped or stepped twice. Times the step and the drawing separately. Returns
        the end reason and (the worst step, the worst drawing and the worst tick, in µs, and the
        overruns: the ticks that ended after the next was due)."""
        recorder, stack, keys, step = self.recorder, self.stack_slot[0], self.keys, jc.step
        step_max = render_max = tick_max = overruns = tick = 0
        due = time.ticks_us()
        while True:
            due = wait_until(due)
            started = time.ticks_us()
            end = step(recorder, sample(keys), stack)
            stepped = time.ticks_us()
            self.render()
            done = time.ticks_us()
            # Tick k + 1 is due floor((k + 1) × 10^6 / 15) - floor(k × 10^6 / 15) µs after tick k:
            # 66,666, 66,667, 66,667, repeating. A running due time keeps ticks_add's delta small.
            due = time.ticks_add(due, _TICK_US + (1 if tick % 3 else 0))
            tick += 1
            step_max = max(step_max, time.ticks_diff(stepped, started))
            render_max = max(render_max, time.ticks_diff(done, stepped))
            tick_max = max(tick_max, time.ticks_diff(done, started))
            if not 0 <= time.ticks_diff(due, done) <= _TICK_US + 1:  # past, or past ticks_diff's range
                overruns += 1
            if tick % _GC_EVERY_TICKS == 0 and time.ticks_diff(done, started) < _GC_LIGHT_US:
                gc.collect()
            if end >= _STEP_ERROR:
                raise RuntimeError("jiro-core error %d on tick %d" % (end - _STEP_ERROR, tick - 1))
            if end:
                return end, (step_max, render_max, tick_max, overruns)

    # Its drawing.

    def redraw(self):
        """Paints every cell and the entities, and pushes the whole screen."""
        jc.status(self.recorder, self.status)  # where the entities are, for render to erase them there
        jc.paint(self.recorder, self.shadow, self.frame, CELL_COLORS, self.rect, True)
        self.render()
        self.screen.show()

    def render(self):
        """Erases the entities where they stood, paints the cells that changed, draws the
        entities, then pushes only the regions that changed: the changed cells, and the cells each
        entity left and entered, the player's with its ring. Nothing is pushed before everything
        is drawn, so the screen never shows a capture's cells without the entities on them. One
        call with few locals, since a call whose frame is over 11 words takes it from the heap, a
        first-fit search that a fragmented heap makes slow, and slower the larger the frame
        (wedgie/scripts/check_app_frames.py)."""
        # The array of the render before last takes this tick's status.
        status, previous = self.previous, self.status
        self.status, self.previous = status, previous
        recorder, frame = self.recorder, self.frame  # a tuple of 4 or more would be built on the heap
        fill, show = self.fill_rect, self.show_rect
        jc.status(recorder, status)
        # The grid's cells again where the entities stood: the boss, the live minions, and the
        # player with the cells its ring reached into.
        jc.restore(recorder, frame, CELL_COLORS, previous[_BOSS_X], previous[_BOSS_Y], previous[_BOSS_SIZE], previous[_BOSS_SIZE])
        for slot in range(_MINIONS, _STATUS_LEN, _MINION_FIELDS):
            if previous[slot] == _LIVE:
                jc.restore(recorder, frame, CELL_COLORS, previous[slot + 1], previous[slot + 2], 1, 1)
        jc.restore(recorder, frame, CELL_COLORS, max(previous[_PLAYER_X] - 1, 0), max(previous[_PLAYER_Y] - 1, 0), 3, 3)
        # The cells that changed since the last render; their rectangle goes to self.rect.
        jc.paint(recorder, self.shadow, frame, CELL_COLORS, self.rect)
        # The entities where they are now, the player on top.
        fill(status[_BOSS_X] * _CELL_PX, status[_BOSS_Y] * _CELL_PX, status[_BOSS_SIZE] * _CELL_PX, status[_BOSS_SIZE] * _CELL_PX, BOSS)
        for slot in range(_MINIONS, _STATUS_LEN, _MINION_FIELDS):
            if status[slot] == _LIVE:
                fill(status[slot + 1] * _CELL_PX, status[slot + 2] * _CELL_PX, _CELL_PX, _CELL_PX, MINION)
        x, y = status[_PLAYER_X] * _CELL_PX, status[_PLAYER_Y] * _CELL_PX  # fill_rect clips the ring to the screen
        fill(x - _PLAYER_MARGIN_PX, y - _PLAYER_MARGIN_PX, _RING_PX, _RING_PX, PLAYER_RING)
        fill(x - 1, y - 1, _RING_PX - 2, _RING_PX - 2, PLAYER_PROTECTED if status[_INVULNERABLE] else PLAYER)
        # The changed cells' rectangle, if any cell changed: its rows at full width when it is wide.
        if self.rect[2] >= _BAND_MIN_PX:
            self.show_band(self.rect[1], self.rect[1] + self.rect[3])
        elif self.rect[2]:
            show(self.rect[0], self.rect[1], self.rect[2], self.rect[3])
        # One push per entity: the rectangle that holds where it stood and where it is.
        x, y = min(previous[_BOSS_X], status[_BOSS_X]), min(previous[_BOSS_Y], status[_BOSS_Y])
        show(x * _CELL_PX, y * _CELL_PX, (max(previous[_BOSS_X], status[_BOSS_X]) + status[_BOSS_SIZE] - x) * _CELL_PX,
             (max(previous[_BOSS_Y], status[_BOSS_Y]) + status[_BOSS_SIZE] - y) * _CELL_PX)
        for slot in range(_MINIONS, _STATUS_LEN, _MINION_FIELDS):
            if previous[slot] == _LIVE or status[slot] == _LIVE:
                old = previous if previous[slot] == _LIVE else status
                new = status if status[slot] == _LIVE else previous
                x, y = min(old[slot + 1], new[slot + 1]), min(old[slot + 2], new[slot + 2])
                show(x * _CELL_PX, y * _CELL_PX, (max(old[slot + 1], new[slot + 1]) + 1 - x) * _CELL_PX,
                     (max(old[slot + 2], new[slot + 2]) + 1 - y) * _CELL_PX)
        x = max(min(previous[_PLAYER_X], status[_PLAYER_X]) * _CELL_PX - _PLAYER_MARGIN_PX, 0)
        y = max(min(previous[_PLAYER_Y], status[_PLAYER_Y]) * _CELL_PX - _PLAYER_MARGIN_PX, 0)
        show(x, y, min(max(previous[_PLAYER_X], status[_PLAYER_X]) * _CELL_PX + _CELL_PX + _PLAYER_MARGIN_PX, _FIELD_PX) - x,
             min(max(previous[_PLAYER_Y], status[_PLAYER_Y]) * _CELL_PX + _CELL_PX + _PLAYER_MARGIN_PX, _FIELD_PX) - y)

    # Its end.

    def finish(self, index, end, timing):
        """The between-level screen: the claim, the score and the lives, and the best, with a
        line more while the bests are not saved; the timing goes to USB only. The score is the
        core's, with the calibration's weight. No line is longer than the screen's 28 characters.
        The screen comes before the USB output, since a print blocks for about 500 ms when a host
        holds the port open without reading it."""
        name, weight, _, _ = self.levels[index]
        status = self.status
        score = jc.score(self.recorder, weight)
        lines = [("claim " + percent(status[_CLAIM]), ui.INK),
                 ("score %d, %s" % (score, self.keep_best(index, score)), ui.INK),
                 ("%s, %d ticks" % (lives(status[_LIVES]), status[_TICKS]), ui.MUTED)]
        if self.unsaved is not None:
            lines.append(("best not saved: " + ("flash full" if self.unsaved == 0 else "errno %d" % self.unsaved), ui.RED))
        ui.page(self.screen, END_TITLES[end], lines, "{g} next")
        print("jiro: level %d %s end %d claim_bp %d ticks %d lives %d score %d" % (
            index + 1, name, end, status[_CLAIM], status[_TICKS], status[_LIVES], score))
        print("jiro: step max %d us, render max %d us, worst tick %d us, overruns %d" % timing)
        self.print_journal()
        self.wait_for_green()

    def print_journal(self):
        """The level's journal over USB, in hex, which `jiro-tools replay` replays. For a host that
        holds the port open without reading it, the lines stop at the first print that takes
        longer than _PRINT_STALL_MS."""
        start = 0
        while True:
            chunk = jc.journal(self.recorder, start, start + _JOURNAL_LINE_BYTES)
            if not chunk:
                break
            printed = time.ticks_ms()
            print("jiro: journal", hexlify(chunk).decode())
            if time.ticks_diff(time.ticks_ms(), printed) > _PRINT_STALL_MS:
                break
            start += _JOURNAL_LINE_BYTES

    # The bests.

    def best_key(self, index):
        """As the browser's Training page keys bests: the rules version, the level id and the
        weight, so a new calibration or rules version starts every level afresh."""
        _, weight, level_id, _ = self.levels[index]
        return "%d/0x%s/%d" % (jc.rules_version(), hexlify(level_id).decode(), weight)

    def keep_best(self, index, score):
        """Keeps `score` if it is the level's best, and stores the bests if it is, or if a store
        failed since the last one that did not: a best that a full flash kept out of the save is
        stored at the next level's end, new best or not. Returns the summary's words for the best."""
        key = self.best_key(index)
        best = self.bests.get(key)
        if best is None or score > best:
            self.bests[key] = score
            words = "a new best"
        else:
            words = "best %d" % best
            if self.unsaved is None:
                return words
        self.unsaved = self.store_bests()
        return words

    def store_bests(self):
        """Stores the bests in the core's stack's block, idle between levels, then takes the stack
        back; returns `store`'s result. save.store opens a 1,148-byte file and writes the bests'
        text, which the heap may hold in no other block by now. Nothing allocated in between
        stays alive, so the block is whole again for the stack: a failed store leaves only its
        errno."""
        slot = self.stack_slot
        size = len(slot[0])
        slot[0] = None
        gc.collect()
        try:
            return store(self.bests)
        finally:
            gc.collect()
            try:
                slot[0] = bytearray(size)
            except MemoryError:
                raise RuntimeError("the core's stack could not be taken back after storing the bests")
