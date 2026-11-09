# Jiro-ikusu's code (Training, README §8.1), which jiro.py runs once it holds the core's buffers. Shipped
# precompiled as jiro_app.mpy (wedgie/scripts/build-app.sh). The core is the referee: each tick's
# inputs go into the README §6.1 mask and through the core's Recorder, and the app only draws
# what the core reports. wedgie/scripts/qemu_app_check.py plays it on MicroPython's qemu port.
import gc
import os
import struct
import time
from array import array
from binascii import hexlify

import jiro_core as jc
import lcd
import save
import ui

MAGIC = b"JL01"
LEVEL_BYTES = 472
TRAINING = 0
TRAINING_INDEX = 0  # Training plays at levelIndex 0, as in the browser
# Tick k is due at t0 + floor(k × 10^6 / 15) µs: 66,666 µs and two thirds, on an absolute schedule.
TICK_US = 66666
OVERLAY_MS = 1500
SUMMARY_GUARD_MS = 500
# The app collects every GC_EVERY_TICKS ticks, after a tick whose work took under GC_LIGHT_US, so
# that the garbage of a tick's drawing (the frames and lists of the firmware's show_rect) is not
# collected inside a heavy tick: a collection took up to 27.5 ms in the app on a wedgie, and a
# light tick and a collection fit in a tick (round E1: no overrun, against three before).
GC_EVERY_TICKS = 8
GC_LIGHT_US = 25000
PRINT_STALL_MS = 100  # a print that takes longer finds the USB port open but not read
CELL_PX = 4
FIELD_PX = 240

# The controls of README §2 (D8) and their bits of the README §6.1 mask. Y and the joystick press
# are not controls, so they never reach the mask.
CONTROL_BITS = {"up": 1, "down": 2, "left": 4, "right": 8, "A": 16, "B": 32, "X": 64}

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
PLAYER_MARGIN_PX = 2
RING_PX = CELL_PX + 2 * PLAYER_MARGIN_PX

# The fields of jc.status (jiro_status in natmod/rust/src/lib.rs), in its order.
END, TICKS, TIME_LIMIT, LIVES, SHIELD, INVULNERABLE, CUTTING, CLAIM, TARGET = range(9)
PLAYER_X, PLAYER_Y, BOSS_X, BOSS_Y, BOSS_SIZE = range(9, 14)
MINIONS = 14  # from here, each of the 3 minion slots' state, then its x and y
STATUS_LEN = 23
LIVE = 1
STEP_ERROR = 0x100
END_TITLES = ("", "Committed", "Cleared", "Out of lives", "Time up")
CLEARED = 2
# A changed area at least this wide is pushed as full-width rows, the screen's fast path: a row of
# show_rect costs about 45 µs plus 0.26 µs a pixel, a row of a full-width band about 75 µs, which
# also allocates nothing per chunk of rows (wedgie.dev/code.md; the firmware's lcd.py).
BAND_MIN_PX = 128


def main(stack, recorder, shadow, level, level_file, bests, heap, stages):
    """`stack` is a list that holds the core's stack, which Training lends to save.store."""
    if jc.recorder_size() > len(recorder) or jc.shadow_size() > len(shadow) or len(level) != LEVEL_BYTES:
        raise ValueError("jiro_core needs a %d B Recorder, a %d B shadow and a %d B level buffer" % (
            jc.recorder_size(), jc.shadow_size(), LEVEL_BYTES))
    if not isinstance(bests, dict):
        raise ValueError("the bests save is not a dict")
    Training(stack, recorder, shadow, level, level_file, bests, heap, stages).run()


def read_levels(f):
    """The level file's index: (name, weightBp, level id, offset of the level bytes) per level."""
    levels = []
    f.seek(0)
    if f.read(4) != MAGIC:
        raise ValueError("the level file is not a JL01 level file")
    rules_version, count = struct.unpack(">HB", f.read(3))
    if rules_version != jc.rules_version():
        raise ValueError("the levels are scored for rules version %d, the core runs %d" % (rules_version, jc.rules_version()))
    offset = 7
    for _ in range(count):
        name_bytes = f.read(1)[0]
        name = f.read(name_bytes).decode()
        weight = struct.unpack(">H", f.read(2))[0]
        level_id = f.read(32)
        offset += 1 + name_bytes + 2 + 32
        levels.append((name, weight, level_id, offset))
        f.seek(LEVEL_BYTES, 1)
        offset += LEVEL_BYTES
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


def left_until(due):
    """The µs left until `due`, or -1 once it has passed. A due time is never more than one tick
    ahead, so one further ahead is more than 2^29 µs late, past ticks_diff's range: it has passed."""
    left = time.ticks_diff(due, time.ticks_us())
    return left if left <= TICK_US + 1 else -1


def wait_until(due):
    left = left_until(due)
    if left > 2000:
        time.sleep_ms(left // 1000 - 1)  # the firmware answers USB in here
    while left_until(due) > 0:
        pass


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
    def __init__(self, stack, recorder, shadow, level, level_file, bests, heap, stages):
        self.stack_holder, self.stack, self.stack_bytes = stack, stack[0], len(stack[0])
        self.recorder, self.shadow, self.level, self.level_file, self.bests = recorder, shadow, level, level_file, bests
        self.heap, self.stages = heap, stages
        self.screen, self.keys = lcd.LCD(), lcd.Keys()
        heap(stages, "lcd")
        self.frame = self.screen.buffer
        self.show_rect, self.fill_rect = self.screen.show_rect, self.screen.fill_rect  # bound once, not per tick
        self.show_band = self.screen.show
        self.status, self.previous = array("H", [0] * STATUS_LEN), array("H", [0] * STATUS_LEN)
        self.rect = array("H", [0] * 4)
        self.levels = read_levels(level_file)
        # Room in the bests for a key per level, made while the heap has room: a new best then
        # never makes the dict grow, into a block the size of its table, at the end of a level,
        # when the heap may have none. MicroPython's dict keeps its table when keys are deleted.
        for key in range(len(self.levels)):
            bests[key] = 0
        for key in range(len(self.levels)):
            del bests[key]
        heap(stages, "app")

    def best_key(self, index):
        """As the browser's Training page keys bests: the rules version, the level id and the
        weight, so a new calibration or rules version starts every level afresh."""
        _, weight, level_id, _ = self.levels[index]
        return "%d/0x%s/%d" % (jc.rules_version(), hexlify(level_id).decode(), weight)

    def run(self):
        index = 0
        while True:
            index = self.choose(index)
            end = self.play(index)
            if end == CLEARED and index + 1 < len(self.levels):
                index += 1
            elif end == CLEARED:
                self.congrats()
                index = 0

    def choose(self, index):
        """The level select, outside play and never journaled: the joystick picks, green plays."""
        count = len(self.levels)
        self.heap(self.stages, "sel")
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
                time.sleep_ms(30)
            index = (index + step) % count

    def start(self, index):
        name, _, level_id, offset = self.levels[index]
        self.level_file.seek(offset)
        if self.level_file.readinto(self.level) != LEVEL_BYTES:
            raise ValueError("the level file ends inside level %s" % name)
        # Not the firmware's wedgie.rand: its first call probes the I2C bus for a secure element and
        # keeps the drivers it imports, about 13 KB of a wedgie's heap (round E1).
        code = jc.start(self.recorder, self.level, TRAINING, os.urandom(32), TRAINING_INDEX, self.stack)
        if code:
            raise RuntimeError("the core rejects level %s: code %d" % (name, code))
        if jc.journal(self.recorder, 36, 68) != level_id:
            raise ValueError("level %s is not the calibration's" % name)

    def redraw(self):
        """Paints every cell and the entities, and pushes the whole screen."""
        jc.paint(self.recorder, self.shadow, self.frame, CELL_COLORS, self.rect, True)
        self.render(self.status, self.status)
        self.screen.show()

    def overlay(self, index):
        """The brief overlay at level start, before tick 0: the level, its target and the lives."""
        s, status = self.screen, self.status
        s.fill_rect(24, 80, 192, 80, ui.WHITE)
        s.center_text(self.levels[index][0], 92, ui.INK, 2)
        s.center_text("target %d %%" % (status[TARGET] // 100), 122, ui.MUTED, 1)
        s.center_text("%d lives" % status[LIVES], 138, ui.MUTED, 1)
        s.show()
        time.sleep_ms(OVERLAY_MS)

    def play(self, index):
        self.start(index)
        self.heap(self.stages, "lvl")
        self.redraw()
        self.overlay(index)
        self.redraw()
        self.keys.pressed()  # presses made before tick 0 are not inputs
        gc.collect()
        end, timing = self.ticks()
        self.finish(index, end, timing)
        return end

    def ticks(self):
        """Steps the level once per tick until it ends, on an absolute schedule: a late tick is
        late, never skipped or stepped twice. Times the step and the drawing separately."""
        recorder, stack, keys, step = self.recorder, self.stack, self.keys, jc.step
        status, previous = self.status, self.previous
        step_max = render_max = tick_max = overruns = 0
        due = time.ticks_us()
        tick = 0
        while True:
            wait_until(due)
            started = time.ticks_us()
            end = step(recorder, sample(keys), stack)
            stepped = time.ticks_us()
            status, previous = previous, status
            self.render(status, previous)
            done = time.ticks_us()
            # Tick k + 1 is due floor((k + 1) × 10^6 / 15) - floor(k × 10^6 / 15) µs after tick k:
            # 66,666, 66,667, 66,667, repeating. A running due time keeps ticks_add's delta small.
            due = time.ticks_add(due, TICK_US + (1 if tick % 3 else 0))
            tick += 1
            step_max = max(step_max, time.ticks_diff(stepped, started))
            render_max = max(render_max, time.ticks_diff(done, stepped))
            tick_max = max(tick_max, time.ticks_diff(done, started))
            if left_until(due) < 0:
                overruns += 1
            if tick % GC_EVERY_TICKS == 0 and time.ticks_diff(done, started) < GC_LIGHT_US:
                gc.collect()
            if end >= STEP_ERROR:
                raise RuntimeError("jiro-core error %d on tick %d" % (end - STEP_ERROR, tick - 1))
            if end:
                self.status, self.previous = status, previous
                return end, (step_max, render_max, tick_max, overruns)

    def render(self, status, previous):
        """Erases the entities where they stood, paints the cells that changed, draws the
        entities, and pushes only the regions that changed: the cells each entity left and
        entered, the player's with its ring. One call with few locals, since a call whose frame
        is over 11 words takes it from the heap, a first-fit search that a fragmented heap makes
        slow, and slower the larger the frame."""
        recorder, frame = self.recorder, self.frame  # a tuple of 4 or more would be built on the heap
        fill, show = self.fill_rect, self.show_rect
        jc.status(recorder, status)
        jc.restore(recorder, frame, CELL_COLORS, previous[BOSS_X], previous[BOSS_Y], previous[BOSS_SIZE], previous[BOSS_SIZE])
        for slot in range(MINIONS, STATUS_LEN, 3):
            if previous[slot] == LIVE:
                jc.restore(recorder, frame, CELL_COLORS, previous[slot + 1], previous[slot + 2], 1, 1)
        jc.restore(recorder, frame, CELL_COLORS, max(previous[PLAYER_X] - 1, 0), max(previous[PLAYER_Y] - 1, 0), 3, 3)
        if jc.paint(recorder, self.shadow, frame, CELL_COLORS, self.rect):
            if self.rect[2] >= BAND_MIN_PX:
                self.show_band(self.rect[1], self.rect[1] + self.rect[3])
            else:
                show(self.rect[0], self.rect[1], self.rect[2], self.rect[3])
        fill(status[BOSS_X] * CELL_PX, status[BOSS_Y] * CELL_PX, status[BOSS_SIZE] * CELL_PX, status[BOSS_SIZE] * CELL_PX, BOSS)
        for slot in range(MINIONS, STATUS_LEN, 3):
            if status[slot] == LIVE:
                fill(status[slot + 1] * CELL_PX, status[slot + 2] * CELL_PX, CELL_PX, CELL_PX, MINION)
        x, y = status[PLAYER_X] * CELL_PX, status[PLAYER_Y] * CELL_PX  # fill_rect clips the ring to the screen
        fill(x - PLAYER_MARGIN_PX, y - PLAYER_MARGIN_PX, RING_PX, RING_PX, PLAYER_RING)
        fill(x - 1, y - 1, RING_PX - 2, RING_PX - 2, PLAYER_PROTECTED if status[INVULNERABLE] else PLAYER)
        x, y = min(previous[BOSS_X], status[BOSS_X]), min(previous[BOSS_Y], status[BOSS_Y])
        show(x * CELL_PX, y * CELL_PX, (max(previous[BOSS_X], status[BOSS_X]) + status[BOSS_SIZE] - x) * CELL_PX,
             (max(previous[BOSS_Y], status[BOSS_Y]) + status[BOSS_SIZE] - y) * CELL_PX)
        for slot in range(MINIONS, STATUS_LEN, 3):
            if previous[slot] == LIVE or status[slot] == LIVE:
                old = previous if previous[slot] == LIVE else status
                new = status if status[slot] == LIVE else previous
                x, y = min(old[slot + 1], new[slot + 1]), min(old[slot + 2], new[slot + 2])
                show(x * CELL_PX, y * CELL_PX, (max(old[slot + 1], new[slot + 1]) + 1 - x) * CELL_PX,
                     (max(old[slot + 2], new[slot + 2]) + 1 - y) * CELL_PX)
        x = max(min(previous[PLAYER_X], status[PLAYER_X]) * CELL_PX - PLAYER_MARGIN_PX, 0)
        y = max(min(previous[PLAYER_Y], status[PLAYER_Y]) * CELL_PX - PLAYER_MARGIN_PX, 0)
        show(x, y, min(max(previous[PLAYER_X], status[PLAYER_X]) * CELL_PX + CELL_PX + PLAYER_MARGIN_PX, FIELD_PX) - x,
             min(max(previous[PLAYER_Y], status[PLAYER_Y]) * CELL_PX + CELL_PX + PLAYER_MARGIN_PX, FIELD_PX) - y)

    def finish(self, index, end, timing):
        """The between-level screen: the claim, the score and the lives, and the best; the
        timing goes to USB only. The score is the core's, with the calibration's weight. The
        screen comes before the USB output, since a print blocks for about 500 ms when a host
        holds the port open without reading it; for that host, the journal's lines stop at the
        first print that takes longer than PRINT_STALL_MS."""
        name, weight, _, _ = self.levels[index]
        status, recorder = self.status, self.recorder
        score = jc.score(recorder, weight)
        claim = "%d.%02d %%" % (status[CLAIM] // 100, status[CLAIM] % 100)
        key = self.best_key(index)
        best = self.bests.get(key)
        if best is None or score > best:
            self.bests[key] = score
            best_line = self.store_bests()
        else:
            best_line = "best %d" % best
        ui.page(self.screen, END_TITLES[end],
                [("claim " + claim, ui.INK),
                 ("score %d, %s" % (score, best_line), ui.INK),
                 ("%d lives, %d ticks" % (status[LIVES], status[TICKS]), ui.MUTED)], "{g} next")
        print("jiro: level %d %s end %d claim_bp %d ticks %d lives %d score %d" % (
            index + 1, name, end, status[CLAIM], status[TICKS], status[LIVES], score))
        print("jiro: step max %d us, render max %d us, worst tick %d us, overruns %d" % timing)
        start = 0
        while True:
            chunk = jc.journal(recorder, start, start + 48)
            if not chunk:
                break
            printed = time.ticks_ms()
            print("jiro: journal", hexlify(chunk).decode())
            if time.ticks_diff(time.ticks_ms(), printed) > PRINT_STALL_MS:
                break
            start += 48
        self.wait_for_green()

    def congrats(self):
        """After the last level's clear, instead of its level select again: the game is won."""
        ui.page(self.screen, "Congrats!", [("visit jiro-ikusu.win", ui.INK)], "{g} level 1")
        self.wait_for_green()

    def wait_for_green(self):
        """Presses made before a screen's first 0.5 s do not skip it."""
        time.sleep_ms(SUMMARY_GUARD_MS)
        self.keys.pressed()
        while "A" not in self.keys.pressed():
            time.sleep_ms(30)

    def store_bests(self):
        """Stores the bests in the core's stack's block, idle between levels, then takes the stack
        back, and returns the summary's line: save.store opens a 1,148-byte file and writes the
        bests' text, which the heap may hold in no other block by now. Nothing allocated in
        between stays alive, so the block is whole again for the stack: a failed store leaves
        only its errno."""
        self.stack = self.stack_holder[0] = None
        gc.collect()
        try:
            errno = store(self.bests)
        finally:
            gc.collect()
            try:
                self.stack = self.stack_holder[0] = bytearray(self.stack_bytes)
            except MemoryError:
                raise RuntimeError("the core's stack could not be taken back after storing the bests")
        if errno is None:
            return "a new best"
        return "best not saved: " + ("flash full" if errno == 0 else "errno %d" % errno)
