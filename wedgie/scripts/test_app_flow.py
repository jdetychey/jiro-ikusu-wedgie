"""The Training app's flow, on the host: wedgie/app/jiro_app.py run by Python against stand-ins
for the wedgie's modules and for jiro_core, whose levels end as each test scripts them. The play
test (qemu_app_check.py) runs the real core on a Cortex-M0, with a wedgie's heap, through levels
that end on a commit. This covers what those levels never reach, and takes a second: a clear and
the level after it, the last level's clear, every end's summary and its best, a store that
fails, the level file's index against the calibration, and every screen's lines against the
screen's 28 columns. It checks no drawing, heap or timing on a wedgie: MicroPython is not Python.

Usage: python3 -I test_app_flow.py [unittest's arguments]
"""

import contextlib
import importlib.util
import io
import json
import sys
import types
import unittest
from pathlib import Path

sys.dont_write_bytecode = True  # importing the app leaves nothing in its folder
REPO = Path(__file__).resolve().parents[2]
APP = REPO / "wedgie" / "app"
CALIBRATION = json.loads((REPO / "seasons" / "calibration-1.json").read_text())
COLS_SMALL = 28  # ui.COLS_SMALL: the characters across a line of ui.page (wedgie.dev/code.md)
PRESS_AFTER_MS = 1000  # a scripted press comes this long after its screen, past the screens' guards
IDLE_MS = 5000  # with no press left to make, the run ends this long after the last screen
COMMITTED, CLEARED, OUT_OF_LIVES, TIME_UP = 1, 2, 3, 4
INK, MUTED, RED = 0, 6, 3  # ui's colors
# The fields of jiro_core.status, as jiro_app.py names them.
END, TICKS, TIME_LIMIT, LIVES, SHIELD, INVULNERABLE, CUTTING, CLAIM, TARGET = range(9)
PLAYER_X, PLAYER_Y, BOSS_X, BOSS_Y, BOSS_SIZE, MINIONS = 9, 10, 11, 12, 13, 14


class Done(Exception):
    """The scripted presses are made, and the app waits for one more."""


class Clock:
    """MicroPython's time: ticks that wrap at 2^30, and a sleep that only moves the clock."""

    PERIOD = 1 << 30

    def __init__(self):
        self.us = 123_456_789

    def ticks_us(self):
        self.us += 20  # a call takes time, so a loop that waits for the clock ends
        return self.us % self.PERIOD

    def ticks_ms(self):
        return self.us // 1000 % self.PERIOD

    def ticks_add(self, ticks, delta):
        return (ticks + delta) % self.PERIOD

    def ticks_diff(self, end, start):
        return (end - start + self.PERIOD // 2) % self.PERIOD - self.PERIOD // 2

    def sleep_ms(self, ms):
        self.us += ms * 1000


class Wedgie:
    """A wedgie as the app sees it: the screen, the keys, the saves, and the core in the app's
    buffers. `outcomes` scripts the levels played, in order: (ticks, end reason, claim in basis
    points, lives left); `presses` the keys pressed on the screens outside play, in order, each a
    second after its screen. The run ends with Done once they are all made. `stalls` gives the
    ticks whose step takes that many µs, `changed` the pixel rectangle of the cells that every
    paint finds changed."""

    def __init__(self, outcomes=(), presses=(), bests=None, store_errors=(), status=None, stalls=None, changed=None):
        self.outcomes, self.presses, self.store_errors = list(outcomes), list(presses), list(store_errors)
        self.bests = {} if bests is None else bests
        self.entities, self.stalls, self.stepped_at = status or {}, stalls or {}, []
        self.changed, self.drawing = changed, []  # the fills and pushes, in order, a list per render
        self.clock = Clock()
        self.pages, self.colors, self.overlays, self.stores, self.stages, self.pushes = [], [], [], [], [], []
        self.playing = None  # the level in play: its bytes, its outcome and the ticks stepped
        self.screen_at = 0
        self.level_file = open(APP / "jiro_levels.bin", "rb")
        self.stack_slot = [bytearray(64)]
        self.app = self.load_app()

    def close(self):
        self.level_file.close()

    def modules(self):
        """The wedgie's modules, by the names the app imports."""
        wedgie = self

        class LCD:
            buffer = bytearray(28800)

            def show(self, y0=0, y1=240):
                wedgie.push(0, y0, 240, y1 - y0)

            def show_rect(self, x, y, w, h):
                wedgie.push(x, y, w, h)

            def fill_rect(self, x, y, w, h, color):
                wedgie.drawing[-1].append(("fill", x, y, w, h))

            def center_text(self, text, y, color, scale=1):
                wedgie.overlays.append(text)

        class Keys:
            def held(self, name):
                return False

            def pressed(self):
                return wedgie.pressed()

        def page(screen, head, lines, hint):
            wedgie.pages.append((head, [text for text, _ in lines], hint))
            wedgie.colors.append([color for _, color in lines])
            wedgie.screen_at = wedgie.clock.us

        return {
            "time": self.clock,
            "micropython": types.SimpleNamespace(const=lambda value: value),
            "lcd": types.SimpleNamespace(LCD=LCD, Keys=Keys, color=lambda r, g, b: (r + g + b) % 16),
            "ui": types.SimpleNamespace(page=page, WHITE=1, INK=INK, MUTED=MUTED, RED=RED),
            "save": types.SimpleNamespace(store=self.store),
            "jiro_core": types.SimpleNamespace(
                rules_version=lambda: CALIBRATION["rulesVersion"], recorder_size=lambda: 64, start=self.start,
                step=self.step, status=self.status, score=self.score, journal=self.journal,
                paint=self.paint,
                restore=lambda recorder, frame, colors, x, y, width, height: None),
        }

    def load_app(self):
        """jiro_app.py, imported with the wedgie's modules in place of Python's."""
        modules = self.modules()
        kept = {name: sys.modules.get(name) for name in modules}
        sys.modules.update(modules)
        try:
            spec = importlib.util.spec_from_file_location("jiro_app", APP / "jiro_app.py")
            app = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(app)
        finally:
            for name, module in kept.items():
                if module is None:
                    del sys.modules[name]
                else:
                    sys.modules[name] = module
        return app

    def run(self):
        """Runs the app until the scripted presses are made; returns what it printed."""
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            try:
                self.app.main(self.stack_slot, bytearray(64), bytearray(3600), bytearray(472), self.level_file,
                              self.bests, self.stages.append)
            except Done:
                pass
        return printed.getvalue()

    # The firmware's side.

    def pressed(self):
        if self.playing or self.clock.us < self.screen_at + PRESS_AFTER_MS * 1000:
            return []
        if self.presses:
            self.screen_at = self.clock.us
            return [self.presses.pop(0)]
        if self.clock.us > self.screen_at + IDLE_MS * 1000:
            raise Done
        return []

    def push(self, x, y, w, h):
        if not (0 <= x and 0 <= y and w > 0 and h > 0 and x + w <= 240 and y + h <= 240):
            raise AssertionError(f"a push off the screen: {(x, y, w, h)}")
        self.pushes.append((x, y, w, h))
        if self.drawing:
            self.drawing[-1].append(("push", x, y, w, h))

    def store(self, name, value):
        if self.stack_slot[0] is not None:
            raise AssertionError("save.store ran while the app held the core's stack")
        if self.store_errors and (error := self.store_errors.pop(0)) is not None:
            raise error
        self.stores.append((name, dict(value)))

    # The core's side.

    def start(self, recorder, level, mode, run_nonce, level_index, stack):
        if stack is not self.stack_slot[0] or len(stack) != 64:
            raise AssertionError("the core started on a stack that is not the slot's")
        self.playing = [bytes(level), self.outcomes.pop(0), 0]
        return 0

    def step(self, recorder, mask, stack):
        ticks, end, _, _ = self.playing[1]
        self.stepped_at.append(self.clock.us)
        self.clock.us += self.stalls.get(self.playing[2], 0)
        self.playing[2] += 1
        if self.playing[2] < ticks:
            return 0
        self.ended, self.playing = self.playing, None
        return end

    def paint(self, recorder, shadow, frame, colors, rect, full=False):
        rect[:] = type(rect)(rect.typecode, self.changed or (0, 0, 0, 0))
        return 1 if self.changed else 0

    def status(self, recorder, out):
        self.drawing.append([])
        level, (_, end, claim, lives), ticks = self.playing or self.ended
        fields = {END: 0 if self.playing else end, TICKS: ticks, LIVES: lives if not self.playing else level[-3],
                  TIME_LIMIT: int.from_bytes(level[-5:-3], "big"), CLAIM: 0 if self.playing else claim,
                  TARGET: int.from_bytes(level[-2:], "big"), PLAYER_X: 30, PLAYER_Y: 59, BOSS_X: 20, BOSS_Y: 20,
                  BOSS_SIZE: level[451], **self.entities}
        for field in range(len(out)):
            out[field] = fields.get(field, 0)

    def score(self, recorder, weight):
        _, (_, end, claim, _), _ = self.ended
        return 0 if end == OUT_OF_LIVES else claim * weight // 10000

    def journal(self, recorder, start=0, end=None):
        level = (self.playing or self.ended)[0]
        ids = {record[-472:]: record[-472 - 32:-472] for record in level_records()}
        return (bytes(36) + ids[level] + bytes(3))[start:end]


def level_records():
    """The level file's records, each ending with its level id and its level bytes."""
    data = (APP / "jiro_levels.bin").read_bytes()
    records, at = [], 7
    for _ in range(data[6]):
        size = 1 + data[at] + 2 + 32 + 472
        records.append(data[at:at + size])
        at += size
    return records


def best_key(level):
    return f"{CALIBRATION['rulesVersion']}/{CALIBRATION['levelIds'][level - 1]}/{CALIBRATION['weightsBp'][level - 1]}"


def score(level, claim):
    return claim * CALIBRATION["weightsBp"][level - 1] // 10000


class Flow(unittest.TestCase):
    def wedgie(self, **script):
        wedgie = Wedgie(**script)
        self.addCleanup(wedgie.close)
        return wedgie

    def play(self, **script):
        wedgie = self.wedgie(**script)
        wedgie.printed = wedgie.run()
        self.assertEqual((wedgie.outcomes, wedgie.presses), ([], []), "the script was not played to its end")
        for head, lines, hint in wedgie.pages:
            for line in lines + [hint]:
                self.assertLessEqual(len(line), COLS_SMALL, f"{line!r} on {head!r} is wider than the screen")
        return wedgie

    def test_level_file_index_is_the_calibrations(self):
        wedgie = self.wedgie()
        levels = wedgie.app.read_levels(wedgie.level_file)
        paths = sorted((REPO / "levels").glob("[0-9][0-9]-*.bin"))
        self.assertEqual(len(levels), 16)
        data = (APP / "jiro_levels.bin").read_bytes()
        for (name, weight, level_id, offset), path, expected_id, expected_weight in zip(
                levels, paths, CALIBRATION["levelIds"], CALIBRATION["weightsBp"]):
            self.assertEqual(name, path.stem.split("-", 1)[1].title())
            self.assertEqual((weight, "0x" + level_id.hex()), (expected_weight, expected_id))
            self.assertEqual(data[offset:offset + 472], path.read_bytes())

    def test_level_select_wraps_and_a_clear_goes_to_the_next_level(self):
        wedgie = self.play(outcomes=[(3, CLEARED, 7600, 5)], presses=["left", "right", "up", "down", "A", "A"])
        heads = [head for head, _, _ in wedgie.pages]
        self.assertEqual(heads, ["Level 1 of 16", "Level 16 of 16", "Level 1 of 16", "Level 16 of 16", "Level 1 of 16",
                                 "Cleared", "Level 2 of 16"])
        self.assertEqual(wedgie.pages[0], ("Level 1 of 16", ["Open", "no best yet"], "< > {g} play"))
        self.assertEqual(wedgie.pages[-1], ("Level 2 of 16", ["Sentry", "no best yet"], "< > {g} play"))

    def test_the_last_levels_clear_goes_to_congrats_then_level_1(self):
        wedgie = self.play(outcomes=[(3, CLEARED, 7500, 3)], presses=["left", "A", "A", "A"])
        self.assertEqual([head for head, _, _ in wedgie.pages],
                         ["Level 1 of 16", "Level 16 of 16", "Cleared", "Congrats!", "Level 1 of 16"])
        self.assertEqual(wedgie.pages[3], ("Congrats!", ["visit jiro-ikusu.win"], "{g} level 1"))

    def test_a_level_not_cleared_stays_selected_under_its_ends_title(self):
        wedgie = self.play(outcomes=[(9, COMMITTED, 1234, 4), (2700, TIME_UP, 5000, 2), (40, OUT_OF_LIVES, 6000, 0)],
                           presses=["right", "A", "A", "A", "A", "A", "A"])
        self.assertEqual([head for head, _, _ in wedgie.pages],
                         ["Level 1 of 16", "Level 2 of 16", "Committed", "Level 2 of 16", "Time up", "Level 2 of 16",
                          "Out of lives", "Level 2 of 16"])
        self.assertEqual(wedgie.pages[2][1], ["claim 12.34 %", f"score {score(2, 1234)}, a new best", "4 lives, 9 ticks"])
        self.assertEqual(wedgie.pages[4][1], ["claim 50.00 %", f"score {score(2, 5000)}, a new best", "2 lives, 2700 ticks"])
        # Out of lives scores 0, whatever the claim.
        self.assertEqual(wedgie.pages[6][1], ["claim 60.00 %", f"score 0, best {score(2, 5000)}", "0 lives, 40 ticks"])
        self.assertIn(f"jiro: level 2 Sentry end 4 claim_bp 5000 ticks 2700 lives 2 score {score(2, 5000)}", wedgie.printed)

    def test_a_best_is_stored_when_it_is_beaten_and_only_then(self):
        wedgie = self.play(outcomes=[(5, COMMITTED, 3000, 5), (5, COMMITTED, 2000, 5), (5, COMMITTED, 3001, 5)],
                           presses=["A", "A", "A", "A", "A", "A"], bests={"an older rules version's": 7})
        self.assertEqual([lines[1] for head, lines, _ in wedgie.pages if head == "Committed"],
                         [f"score {score(1, 3000)}, a new best", f"score {score(1, 2000)}, best {score(1, 3000)}",
                          f"score {score(1, 3001)}, a new best"])
        self.assertEqual(wedgie.stores, [("bests", {"an older rules version's": 7, best_key(1): score(1, 3000)}),
                                         ("bests", {"an older rules version's": 7, best_key(1): score(1, 3001)})])
        self.assertEqual(wedgie.pages[-1][1], ["Open", f"best {score(1, 3001)}"])
        self.assertEqual(len(wedgie.stack_slot[0]), 64, "the app took the core's stack back after each store")

    def test_a_best_that_a_store_failed_to_save_is_stored_at_the_next_end(self):
        for error, words in ((OSError("flash full: delete some saves at wedgie.dev"), "flash full"),
                             (OSError(28, "ENOSPC"), "errno 28")):
            with self.subTest(words):
                # Level 16's highest score, then a lower one twice: no new best, but one to save.
                wedgie = self.play(outcomes=[(5, COMMITTED, 10000, 5), (5, COMMITTED, 100, 3), (5, COMMITTED, 100, 3)],
                                   presses=["left", "A", "A", "A", "A", "A", "A"], store_errors=[error, error])
                summaries = [(lines, colors) for (head, lines, _), colors in zip(wedgie.pages, wedgie.colors)
                             if head == "Committed"]
                best = score(16, 10000)
                self.assertEqual(summaries[0], (["claim 100.00 %", f"score {best}, a new best", "5 lives, 5 ticks",
                                                 f"best not saved: {words}"], [INK, INK, MUTED, RED]))
                self.assertEqual(summaries[1], (["claim 1.00 %", f"score {score(16, 100)}, best {best}", "3 lives, 5 ticks",
                                                 f"best not saved: {words}"], [INK, INK, MUTED, RED]))
                self.assertEqual(summaries[2], (["claim 1.00 %", f"score {score(16, 100)}, best {best}", "3 lives, 5 ticks"],
                                                [INK, INK, MUTED]))
                self.assertEqual(wedgie.stores, [("bests", {best_key(16): best})])
                self.assertEqual(len(wedgie.stack_slot[0]), 64, "the app took the core's stack back after each store")

    def test_the_overlay_names_the_level_its_target_and_its_lives(self):
        wedgie = self.play(outcomes=[(3, COMMITTED, 0, 5)], presses=["A", "A"])
        self.assertEqual(wedgie.overlays, ["Open", "target 75 %", "5 lives"])

    def test_the_overlay_gives_every_levels_target_to_the_basis_point(self):
        # Two levels' targets are not whole percentages: Ring's 7,650 and Titan's 8,150 basis points.
        fractional = {}
        for level, record in enumerate(level_records(), 1):
            target, lives = int.from_bytes(record[-2:], "big"), record[-3]
            wedgie = self.play(outcomes=[(3, COMMITTED, 0, lives)], presses=["right"] * (level - 1) + ["A", "A"])
            name, shown, shown_lives = wedgie.overlays
            self.assertEqual(shown_lives, f"{lives} lives")
            self.assertEqual(float(shown.removeprefix("target ").removesuffix(" %")) * 100, target, name)
            if target % 100:
                fractional[name] = shown
        self.assertEqual(fractional, {"Ring": "target 76.50 %", "Titan": "target 81.50 %"})

    def test_one_life_is_not_1_lives(self):
        wedgie = self.play(outcomes=[(9, COMMITTED, 1234, 1)], presses=["A", "A"])
        self.assertEqual(wedgie.pages[1][1][2], "1 life, 9 ticks")

    def test_the_stages_of_the_heap_are_measured(self):
        wedgie = self.play(outcomes=[(3, COMMITTED, 0, 5)], presses=["A", "A"])
        self.assertEqual(wedgie.stages, ["lcd", "app", "sel", "lvl", "sel"])

    def test_the_journal_is_printed_in_lines_of_48_bytes(self):
        wedgie = self.play(outcomes=[(3, COMMITTED, 0, 5)], presses=["A", "A"])
        journal = [line.split()[2] for line in wedgie.printed.splitlines() if line.startswith("jiro: journal ")]
        self.assertEqual([len(line) // 2 for line in journal], [48, 23])
        self.assertEqual(bytes.fromhex("".join(journal))[36:68].hex(), CALIBRATION["levelIds"][0][2:])

    def test_a_tick_pushes_within_the_screen_wherever_the_entities_are(self):
        edges = (0, 1, 30, 58, 59)
        for x in edges:
            for y in edges:
                entities = {PLAYER_X: x, PLAYER_Y: y, BOSS_X: min(x, 52), BOSS_Y: min(y, 52), MINIONS: 1,
                            MINIONS + 1: y, MINIONS + 2: x, MINIONS + 3: 2, MINIONS + 6: 1, MINIONS + 7: 59 - x,
                            MINIONS + 8: 59 - y}
                with self.subTest(player=(x, y)):
                    wedgie = self.play(outcomes=[(3, COMMITTED, 0, 5)], presses=["A", "A"], status=entities)
                    # Each render pushes the boss, the two live minions and the player.
                    self.assertEqual(len([push for push in wedgie.pushes if push[2:] != (240, 240)]) % 4, 0)

    def test_a_tick_draws_everything_before_it_pushes_anything(self):
        # Changed cells 16 pixels wide go out as their rectangle, 128 wide as their rows at full width.
        for changed, pushed in (((40, 80, 16, 24), (40, 80, 16, 24)), ((40, 80, 128, 24), (0, 80, 240, 24))):
            with self.subTest(changed=changed):
                wedgie = self.play(outcomes=[(3, COMMITTED, 0, 5)], presses=["A", "A"], changed=changed)
                renders = [drawn for drawn in wedgie.drawing if drawn]
                self.assertEqual(len(renders), 2 + 3, "the two redraws of a level start, then a render a tick")
                for drawn in renders[2:]:
                    # The boss, the player's ring and the player; then the changed cells, the boss and the player.
                    self.assertEqual([kind for kind, *_ in drawn], ["fill"] * 3 + ["push"] * 3)
                    self.assertEqual(drawn[3], ("push", *pushed))

    def test_a_tick_that_changed_no_cell_pushes_the_entities_alone(self):
        wedgie = self.play(outcomes=[(3, COMMITTED, 0, 5)], presses=["A", "A"])
        for drawn in [drawn for drawn in wedgie.drawing if drawn][2:]:
            self.assertEqual([kind for kind, *_ in drawn], ["fill"] * 3 + ["push"] * 2, "the boss, then the player")

    def test_the_core_refusing_a_step_or_a_level_raises(self):
        with self.assertRaisesRegex(RuntimeError, "jiro-core error 15 on tick 2"):
            self.wedgie(outcomes=[(3, 0x100 + 15, 0, 5)], presses=["A"]).run()
        wedgie = self.wedgie(outcomes=[(3, COMMITTED, 0, 5)], presses=["A"])
        wedgie.start = lambda *args: 36
        wedgie.app = wedgie.load_app()
        with self.assertRaisesRegex(RuntimeError, "the core rejects level Open: code 36"):
            wedgie.run()

    def test_a_level_whose_id_is_not_the_calibrations_raises(self):
        wedgie = self.wedgie(outcomes=[(3, COMMITTED, 0, 5)], presses=["A"])
        wedgie.journal = lambda recorder, start=0, end=None: bytes(71)[start:end]
        wedgie.app = wedgie.load_app()
        with self.assertRaisesRegex(ValueError, "level Open is not the calibration's"):
            wedgie.run()

    def test_a_save_that_is_not_a_dict_or_a_recorder_too_small_raises(self):
        with self.assertRaisesRegex(ValueError, "the bests save is not a dict"):
            self.wedgie(bests=[1, 2]).run()
        wedgie = self.wedgie()
        with self.assertRaisesRegex(ValueError, "jiro_core needs a 64 B Recorder and a 472 B level buffer"):
            wedgie.app.main(wedgie.stack_slot, bytearray(63), bytearray(3600), bytearray(472), wedgie.level_file, {},
                            wedgie.stages.append)


class Ticks(unittest.TestCase):
    """The tick's clock: 15 ticks a second, a late tick made up for by the next, and a tick more
    than one tick late not made up for at all."""

    TICK_US = 1_000_000 / 15
    NEAR_US = 400  # the stand-in clock moves 20 µs a reading

    def setUp(self):
        self.wedgie = Wedgie()
        self.addCleanup(self.wedgie.close)
        self.app, self.clock = self.wedgie.app, self.wedgie.clock

    def test_wait_until_returns_the_due_time_at_the_due_time(self):
        for ahead in (0, 1, 1999, 2001, 66666, 66667):
            with self.subTest(ahead=ahead):
                due = self.clock.ticks_add(self.clock.ticks_us(), ahead)
                self.assertEqual(self.app.wait_until(due), due)
                self.assertTrue(0 <= self.clock.ticks_diff(self.clock.ticks_us(), due) < self.NEAR_US)

    def test_wait_until_runs_a_tick_up_to_one_tick_late_at_once_on_its_due_time(self):
        for late in (1, 30_000, 66_600):  # the stand-in clock moves on while wait_until reads it
            with self.subTest(late=late):
                due = self.clock.ticks_add(self.clock.ticks_us(), -late)
                before = self.clock.us
                self.assertEqual(self.app.wait_until(due), due)
                self.assertLess(self.clock.us - before, self.NEAR_US)

    def test_wait_until_starts_the_schedule_again_from_a_tick_later_than_that(self):
        # The last two are past ticks_diff's range of 2^29 µs, where a late due time reads as ahead.
        for late in (66_800, 10_000_000, (1 << 29) - 5, (1 << 29) + 5, 600_000_000):
            with self.subTest(late=late):
                due = self.clock.ticks_add(self.clock.ticks_us(), -late)
                before = self.clock.us
                now = self.app.wait_until(due)
                self.assertLess(self.clock.us - before, self.NEAR_US)
                self.assertTrue(0 <= self.clock.ticks_diff(self.clock.ticks_us(), now) < self.NEAR_US)

    def test_wait_until_waits_across_the_clocks_wrap(self):
        self.clock.us = Clock.PERIOD - 30_000
        due = self.clock.ticks_add(self.clock.ticks_us(), 60_000)
        self.assertEqual(self.app.wait_until(due), due)
        self.assertTrue(0 <= self.clock.ticks_diff(self.clock.ticks_us(), due) < self.NEAR_US)

    def level(self, ticks, stalls=None):
        """The µs between the steps of a level of `ticks` ticks, and its overruns as printed."""
        wedgie = Wedgie(outcomes=[(ticks, COMMITTED, 0, 5)], presses=["A", "A"], stalls=stalls)
        self.addCleanup(wedgie.close)
        printed = wedgie.run()
        steps = wedgie.stepped_at
        self.assertEqual(len(steps), ticks)
        return [later - sooner for sooner, later in zip(steps, steps[1:])], int(printed.split("overruns ")[1].split()[0])

    def test_a_level_runs_at_15_ticks_a_second(self):
        gaps, overruns = self.level(91)
        self.assertEqual(overruns, 0)
        self.assertAlmostEqual(sum(gaps), 90 * self.TICK_US, delta=self.NEAR_US)
        for gap in gaps:
            self.assertAlmostEqual(gap, self.TICK_US, delta=self.NEAR_US)

    def test_the_tick_after_a_late_one_comes_sooner_and_the_schedule_holds(self):
        gaps, overruns = self.level(31, stalls={10: 100_000})
        self.assertEqual(overruns, 1)
        self.assertAlmostEqual(gaps[10], 100_000, delta=self.NEAR_US)  # the next tick runs at once
        self.assertAlmostEqual(gaps[11], 2 * self.TICK_US - 100_000, delta=self.NEAR_US)
        self.assertAlmostEqual(sum(gaps), 30 * self.TICK_US, delta=self.NEAR_US)

    def test_a_stall_of_more_than_a_tick_is_not_made_up_for(self):
        for stall in (150_000, 5_000_000, 600_000_000):
            with self.subTest(stall=stall):
                gaps, overruns = self.level(31, stalls={10: stall})
                self.assertEqual(overruns, 1)
                self.assertAlmostEqual(gaps[10], stall, delta=self.NEAR_US)  # the next tick runs at once
                for gap in gaps[:10] + gaps[11:]:  # and every other tick takes its fifteenth of a second
                    self.assertAlmostEqual(gap, self.TICK_US, delta=self.NEAR_US)


if __name__ == "__main__":
    unittest.main()
