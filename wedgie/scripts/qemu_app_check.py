"""The Training app's play test (plan M3.9): runs wedgie/app/jiro.py's run(), unmodified, on
MicroPython v1.29.0's qemu MICROBIT port (scripts/build-qemu.sh builds it), with the wedgie's
lcd, ui, save, wedgie and time modules and os.urandom stubbed and the app's files served from a RAM filesystem
whose open() allocates what a wedgie's does: a 1,148-byte littlefs file object. The test's files
and stubs sit above one large free block, as a wedgie's free RAM is, and a ballast fills the heap
down to the given free bytes, the free RAM a wedgie reports with no app, before run() starts:
part of them in small holes of 64 to 512 bytes between live blocks, the rest in that block. At
the app's first level select, once it has started, the test fills every free block larger than
SQUEEZE_BYTES, as a wedgie's heap was after round E1's second build started: from then on the app
must allocate nothing larger, so a level start that opens a file fails, as it did on the wedgie
(it fills a block down to SQUEEZE_BYTES, or a block under twice that whole: allocation is first
fit, so a smaller filler would land in a smaller block). There too it wraps the app's sample,
jiro_core's step and the app's render in calls that lock the heap, so a tick that allocates fails
with a MemoryError (the wedgie's own lcd module allocates in show_rect and Keys.pressed, which the
stubs here do not). The scripted inputs pick level 1, play it with two captures and commit, then
from the summary go back to the level select, pick level 2 and play it with the same inputs, so
that what the app allocates per level is allocated twice. The first save.store raises
OSError("flash full: ..."), as a wedgie's does when its flash is full, after allocating the bests'
text and a file object: the app must still take its stack back and play level 2, whose store
saves both bests. The app's clock is a stub over the port's: its ticks_us
wraps 2^30 µs shortly after each level's tick 0, and on tick LATE_TICK it jumps LATE_US ahead,
more than ticks_diff's 2^29 µs range, as a wedgie whose ticks fell that far behind would see it:
the remaining ticks must run at once, late, instead of sleeping for minutes. Then it checks:

- per level, the journal the app printed: its header (Training, the stub's run nonce, the level's
  id from seasons/calibration-1.json, levelIndex 0) and its events, which must be the changes of
  the README §6.1 mask that the scripted keys give under D8, Y and the joystick press never in it;
- per level, the app's end reason, claim, ticks and score against native replay of that journal
  (`jiro-tools replay` with the level's weight), and that it captured something;
- the best scores it stored, the free heap and largest free block it printed at each stage, a
  summary screen per level, the first saying its best was not saved, and no overrun before the
  clock's jump and one on every tick from it on;
- that the heap was locked over every tick's sample, step and render.

With --hard-irq, on the firmware `build-qemu.sh irqsim` builds, every tick of the port's SysTick,
from before the ballast on, also calls a Python callback as a hard IRQ, as the wedgie firmware's
key IRQ (power.py's _touch, on any key press) runs wherever the app is: all the checks above must
still pass, and the callback must have interrupted jiro_core and run cleanly every time. In round
E1 such an interrupt, on the stack jiro_core ran on, froze the wedgie.

It prints the app's step, render and tick maxima in qemu microseconds and in instructions. qemu
runs with -icount shift=3, 8 ns of virtual time per instruction, and the port's ticks assume a
25 MHz SysTick that qemu's nRF51 clocks at 16 MHz, so one microsecond the app measures is
1,562.5 / 8 = 195.3 instructions (measured with an inline-assembler loop of known length).

Usage: python3 -I qemu_app_check.py <firmware.elf> <app dir> <free bytes> [<holes>] [--hard-irq]

<holes> is the number of small holes among the free bytes, HOLES by default; 0 leaves them all in
the large block.

Run it under a timeout: a module that hard-faults hangs qemu. Exit status 0 when every check
passes.
"""

import ast
import base64
import importlib.util
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

spec = importlib.util.spec_from_file_location("qemu_natmod_vectors", Path(__file__).parent / "qemu_natmod_vectors.py")
qemu_natmod_vectors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(qemu_natmod_vectors)

ICOUNT_SHIFT = 3
INSTRUCTIONS_PER_US = 1562.5 / 2 ** ICOUNT_SHIFT
RP2040_MHZ = 125
ROW_PUSH_US = 45  # a show_rect row on a wedgie (wedgie.dev/code.md)
RUN_NONCE = bytes.fromhex("277b9e3cebd776c6d46a8b4eb9b83ac165961f4b46a79baef5df1514bb5219cd")
HEADER_LEN = 71
PLAYS = (1, 2)  # the levels played, in order, from the level select
# MicroPython v1.29.0's rp2 port mounts littlefs with progsize=256, so a file's cache is 1,024
# bytes, allocated with its 124-byte object (extmod/vfs_lfsx_file.c): the wedgie's "memory
# allocation failed, allocating 1148 bytes" of round E1's second try.
LFS_FILE_BYTES = 1148
HOLES = 32
SQUEEZE_BYTES = 1024  # round E1's second build measured a largest free block of 747 B after start on qemu
HOLE_SIZES = (64, 256, 448, 128, 320, 512, 192, 384)  # bytes, interleaved with 16-byte live blocks
UPLOAD_ROOM = 32 * 1024  # what uploading the files needs beside them
STUB_ROOM = 64 * 1024  # what importing the stubs needs, their 28,800-byte screen buffer included
STAGES = ("run", "buf", "imp", "lcd", "app", "sel", "lvl")
WRAP_AFTER_US = 1_500_000  # from the last press outside play, just before tick 0, to the clock's wrap: about tick 22
LATE_TICK = 70
LATE_US = 600_000_000

# README §6.1 bits of the wedgie's key names (D8); Y and press are not controls.
BITS = {"up": 1, "down": 2, "left": 4, "right": 8, "A": 16, "B": 32, "X": 64}
# (ticks, keys held, keys tapped) from the player's start at (30, 59), the bottom rail.
SCRIPT = [
    (5, (), ()),
    (3, ("Y", "press"), ()),            # never in the mask
    (20, ("left",), ()),                # along the rail to (10, 59)
    (4, ("up",), ()),                   # a cut up to (10, 55)
    (8, ("left", "A"), ()),             # a fast cut that closes on the left border: a capture
    (1, (), ("B",)),                    # a tap shorter than a tick
    (15, ("up",), ()),                  # along the left border to (0, 40)
    (12, ("right",), ()),               # a cut to (12, 40)
    (19, ("down",), ()),                # closes on the bottom border: a second capture
    (1, ("X",), ()),                    # commit
]


def menu_presses():
    """The presses outside play, in order: on the level select, joystick right to the level and
    green; the presses discarded before tick 0; and after a level, on its summary, the presses
    discarded in its first 0.5 s, then green."""
    presses, selected = [], 1
    for play, level in enumerate(PLAYS):
        if play:
            presses += [[], ["A"]]
        presses += [["right"]] * (level - selected) + [["A"], []]
        selected = level
    return presses


STUBS = {
    # The screen buffer, allocated before lcd.py, whose scripted inputs take room to compile.
    "lcd_buffer.py": "BUF = bytearray(28800)\n",
    "lcd.py": """\
import clock, framebuf, gc, micropython, sys
from lcd_buffer import BUF as _BUF
_PALETTE = ((0, 0, 0), (255, 255, 255), (40, 100, 230), (255, 42, 42), (255, 220, 0), (60, 255, 120),
            (154, 154, 154), (34, 196, 82), (90, 90, 90))
def color(r, g, b):
  return min(range(len(_PALETTE)), key=lambda i: sum((a - b) ** 2 for a, b in zip(_PALETTE[i], (r, g, b))))
_rows = [0, 0]
class LCD(framebuf.FrameBuffer):
  def __init__(s):
    super().__init__(_BUF, 240, 240, framebuf.GS4_HMSB)
    s.buffer = _BUF
  def show(s, y0=0, y1=240):
    if not 0 <= y0 < y1 <= 240:
      raise ValueError('show off the screen: %r' % ((y0, y1),))
    _rows[0] += y1 - y0
  def show_rect(s, x, y, w, h):
    if not (0 <= x and 0 <= y and w > 0 and h > 0 and x + w <= 240 and y + h <= 240):
      raise ValueError('show_rect off the screen: %r' % ((x, y, w, h),))
    _rows[0] += h
  def center_text(s, t, y, c, scale=1):
    s.text(t, (240 - 8 * len(t)) // 2, y, c)
class ScriptDone(Exception):
  pass
_menu, _script, _tick, _play, _wrap, _level, _held = MENU, SCRIPT, [0], [False], [0], [0], [None] * 64
def _largest():
  gc.collect()
  low, high = 0, gc.mem_free()
  while high - low > 16:
    size = (low + high) // 2
    try:
      block = bytearray(size)
    except MemoryError:
      high = size
    else:
      del block
      low = size
  return low
_inner, _locked = {}, [0, 0, 0]
def _sample(keys):
  _locked[0] += 1
  micropython.heap_lock()
  try:
    return _inner['sample'](keys)
  finally:
    micropython.heap_unlock()
def _step(recorder, mask, stack):
  _locked[1] += 1
  micropython.heap_lock()
  try:
    return _inner['step'](recorder, mask, stack)
  finally:
    micropython.heap_unlock()
def _render(app, status, previous):
  _locked[2] += 1
  micropython.heap_lock()
  try:
    return _inner['render'](app, status, previous)
  finally:
    micropython.heap_unlock()
def _lock_ticks():
  app, jc = sys.modules['jiro_app'], sys.modules['jiro_core']
  _inner['sample'], _inner['step'], _inner['render'] = app.sample, jc.step, app.Training.render
  app.sample, jc.step, app.Training.render = _sample, _step, _render
def _squeeze():
  for i in range(len(_held)):
    size = _largest()
    if size <= SQUEEZE_BYTES:
      break
    _held[i] = bytearray(size - SQUEEZE_BYTES if size > 2 * SQUEEZE_BYTES else size)
  size = _largest()
  if size > SQUEEZE_BYTES:
    raise ValueError('the squeeze left a %d B block' % size)
  print('squeezed: free %d B, largest block %d B' % (gc.mem_free(), size))
class Keys:
  def held(s, name):
    _play[0] = True
    return _tick[0] < len(_script) and name in _script[_tick[0]][0]
  def pressed(s):
    if _play[0] and _tick[0] >= len(_script):
      _level[0] += 1
      if _level[0] == PLAY_COUNT:
        raise ScriptDone('scripted inputs done; most rows pushed in a tick: %d; heap locked over %d samples, %d steps '
                         'and %d renders' % (_rows[1], _locked[0], _locked[1], _locked[2]))
      _play[0], _tick[0] = False, 0
    if not _play[0]:
      if not _inner:
        _lock_ticks()
        _squeeze()
      clock.wrap_in(WRAP_AFTER_US)
      return _menu.pop(0) if _menu else []
    if _tick[0] == 0:
      _wrap[0] = clock.ticks_us()
    if _tick[0] == LATE_TICK:
      if clock.ticks_us() >= _wrap[0]:
        raise ValueError('the clock did not wrap between tick 0 and tick %d' % LATE_TICK)
      clock.jump(LATE_US)
    if _tick[0]:
      _rows[1] = max(_rows[1], _rows[0])  # what the previous tick pushed
    _rows[0] = 0
    _tick[0] += 1
    return _script[_tick[0] - 1][1]
""",
    # Python's ints stay below 2^30 here, small ints on the port: ticks_us is (port's - _back) mod 2^30.
    "clock.py": """\
import time as _time
_back = [0]
def wrap_in(us):
  _back[0] = (_time.ticks_us() - (0x3fffffff - us)) & 0x3fffffff
def ticks_us():
  return (_time.ticks_us() - _back[0]) & 0x3fffffff
def jump(us):
  _back[0] = (_back[0] - us) & 0x3fffffff
ticks_add, ticks_diff, ticks_ms, sleep_ms = _time.ticks_add, _time.ticks_diff, _time.ticks_ms, _time.sleep_ms
""",
    "ui.py": """\
WHITE, INK, MUTED, GREEN, GREEN_D, GREY, RED = 1, 0, 6, 7, 7, 8, 3
def page(screen, head, lines, hint):
  print('ui.page', repr(head), repr([line[0] for line in lines]), repr(hint))
""",
    # As a wedgie's save does: a file to read, or the text of the value and a file to write it to.
    # The test starts with no saves, and the saves are in flash, not in RAM. The first store
    # fails as on a wedgie whose flash is full.
    "save.py": """\
_stores = [0]
def load(name, default):
  _file = bytearray(LFS_FILE_BYTES)
  return default
def store(name, value):
  text = repr(value)
  _file = bytearray(LFS_FILE_BYTES)
  _stores[0] += 1
  if _stores[0] == 1:
    raise OSError('flash full: delete some saves at wedgie.dev')
  print('save.store', repr(name), text)
""",
    # The firmware's wedgie module: its rand() probes the I2C bus for a secure element on its first
    # call and keeps the drivers it imports, about 13 KB of a wedgie's heap (round E1).
    "wedgie.py": """\
def rand(n):
  raise AssertionError('wedgie.rand keeps about 13 KB of a wedgie heap: use os.urandom')
rand_source = rand
""",
    # The app's os: the run nonce comes from os.urandom.
    "os_stub.py": """\
def urandom(n):
  return NONCE[:n]
""",
}

# A RAM filesystem over _FILES, with the seek the app's level reads need.
VFS = """\
import sys, io, vfs, os
class _F(io.IOBase):
  def __init__(s, d): s.lfs = bytearray(LFS_FILE_BYTES); s.d = d; s.o = 0
  def __enter__(s): return s
  def __exit__(s, *a): pass
  def ioctl(s, r, a): return 0 if r == 4 else -1
  def readinto(s, b):
    n = min(len(b), len(s.d) - s.o); b[:n] = memoryview(s.d)[s.o:s.o + n]; s.o += n; return n
  def read(s, n=-1):
    n = len(s.d) - s.o if n < 0 else min(n, len(s.d) - s.o); s.o += n; return bytes(memoryview(s.d)[s.o - n:s.o])
  def seek(s, o, w=0):
    s.o = o if w == 0 else s.o + o if w == 1 else len(s.d) + o; return s.o
class _FS:
  def mount(s, r, m): pass
  def umount(s): pass
  def chdir(s, p): pass
  def getcwd(s): return ''
  def stat(s, p):
    if p.lstrip('/') in _FILES: return (0x8000, 0, 0, 0, 0, 0, len(_FILES[p.lstrip('/')]), 0, 0, 0)
    raise OSError(2)
  def open(s, p, m):
    if p.lstrip('/') not in _FILES: raise OSError(2)
    return _F(_FILES[p.lstrip('/')])
vfs.mount(_FS(), '/ram'); os.chdir('/ram'); sys.path.insert(0, '/ram')
"""

# Before the test's files, a placeholder takes the bottom of the heap, so that they land above it.
# Before the stubs, it shrinks to leave them room to compile, above it too. Freed, it leaves one
# large block below them, as a wedgie's free RAM is. Allocation is first fit.
PLACEHOLDER = """\
import gc
_P0 = None
gc.collect()
_P0 = bytearray(gc.mem_free() - {room})
"""

# The ballast frees the placeholder, fills every free block, largest first (lcd.py's _largest
# measures them), then frees the largest: there the holes' blocks, each followed by a 16-byte
# bytearray that stays, take its bottom, and a filler trims it, freeing the next largest blocks
# first if it is too small. Freeing the holes' blocks then leaves about `free` bytes: `hole bytes`
# in the holes, the rest above them. A bytearray of n bytes is a 16-byte object and its n bytes,
# in 16-byte blocks.
BALLAST = """\
import gc
from lcd import _largest
_P0 = None
_R, _H, _K = [None] * 256, [None] * {holes}, [None] * {holes}
n = 0
while n < len(_R):
  size = _largest()
  if size < 64:
    break
  _R[n] = bytearray(size); n += 1
_R[0] = None
gc.collect()
for i in range({holes}):
  _H[i] = bytearray({hole_sizes}[i % {size_count}]); _K[i] = bytearray(16)
top, k = {free} - {hole_bytes}, 1
while gc.mem_free() < top + 32 and k < n:
  _R[k] = None; k += 1
  gc.collect()
_T = bytearray(max(1, gc.mem_free() - top - 32))
_H = None
gc.collect()
print('heap free with ballast', gc.mem_free(), 'of which', {hole_bytes}, 'in', {holes}, 'holes of 64 to 512 B;',
      'largest block', _largest())
"""

# The wedgie firmware's key IRQ callback, power.py's _touch, which irqsim calls at every tick. Armed
# before the ballast, so that the ballast leaves the app the same free bytes as without it.
ARM_HARD_IRQ = """\
import irqsim
_last = 0
def _touch(_):
  global _last
  _last = clock.ticks_ms()
irqsim.hard(_touch)
"""


def upload_files(repl, files, chunk=3000):
    # Every buffer is allocated before any is filled, so the base64 chunks don't fragment the heap.
    repl.exec("_FILES.update({" + ", ".join(f"'{name}': bytearray({len(data)})" for name, data in files.items()) + "})")
    for name, data in files.items():
        for off in range(0, len(data), chunk):
            part = data[off:off + chunk]
            repl.exec(f"import binascii; _FILES['{name}'][{off}:{off + len(part)}] = "
                      f"binascii.a2b_base64('{base64.b64encode(part).decode()}')")


def run_app(repl):
    """Runs the app until the stub's scripted inputs end; returns its output and its error."""
    repl.p.stdin.write(b"import jiro; jiro.run()\x04")
    repl.p.stdin.flush()
    if repl.read_until(b"OK") != b"":
        raise SystemExit("raw REPL did not acknowledge the code")
    out, err = repl.read_until(b"\x04").decode(), repl.read_until(b"\x04").decode()
    repl.read_until(b">")
    return out, err


def scripted_masks():
    masks = []
    for ticks, held, tapped in SCRIPT:
        masks += [sum(BITS.get(name, 0) for name in set(held) | set(tapped))] * ticks
    return masks


def expected_events(ticks_used):
    events, mask = b"", 0
    for tick, held in enumerate(scripted_masks()[:ticks_used]):
        if held != mask:
            events += tick.to_bytes(2, "big") + bytes([held])
            mask = held
    return events


def native_replay(repo, level_path, journal, weight):
    with tempfile.TemporaryDirectory() as scratch:
        path = Path(scratch) / "level.jrn"
        path.write_bytes(journal)
        result = subprocess.run(["cargo", "run", "-q", "-p", "jiro-tools", "--locked", "--", "replay", "--level",
                                 str(level_path), "--journal", str(path), "--weight", str(weight)],
                                cwd=repo, capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(f"FAIL: native replay of the app's journal: {result.stderr.strip()}")
    out = result.stdout
    return {line.split()[0]: int(line.split()[1]) for line in out.splitlines()
            if line.split()[0] in ("endReason", "claimBp", "ticksUsed", "levelScore")}


def fields(line, *names):
    words = line.split()
    return [int(words[words.index(name) + 1]) for name in names]


def level_outputs(lines):
    """Per level the app ended, in order: its level line, its timing line and its journal."""
    outputs = []
    for at, line in enumerate(lines):
        if line.startswith("jiro: level "):
            journal = ""
            for following in lines[at + 2:]:
                if not following.startswith("jiro: journal "):
                    break
                journal += following.split()[2]
            outputs.append((line, lines[at + 1], bytes.fromhex(journal)))
    return outputs


def check_level(repo, calibration, level, output, stored):
    """The problems of one level's play, and prints its result and timing."""
    line, timing, journal = output
    level_id, weight = bytes.fromhex(calibration["levelIds"][level - 1][2:]), calibration["weightsBp"][level - 1]
    level_path = sorted((repo / "levels").glob("[0-9][0-9]-*.bin"))[level - 1]
    problems = []
    if fields(line, "level")[0] != level or not timing.startswith("jiro: step max"):
        return [f"expected level {level}'s result and timing, got {line!r} and {timing!r}"]
    end, claim, ticks, score = fields(line, "end", "claim_bp", "ticks", "score")
    replayed = native_replay(repo, level_path, journal, weight)
    if [end, claim, ticks, score] != [replayed[key] for key in ("endReason", "claimBp", "ticksUsed", "levelScore")]:
        problems.append(f"the app shows end {end}, claim {claim}, ticks {ticks}, score {score}; native replay {replayed}")
    if claim == 0:
        problems.append("the scripted play captured nothing")
    header = (bytes([1]) + calibration["rulesVersion"].to_bytes(2, "big") + bytes([0]) + RUN_NONCE + level_id
              + bytes([0]) + (len(expected_events(ticks)) // 3).to_bytes(2, "big"))
    if journal[:HEADER_LEN] != header:
        problems.append(f"journal header {journal[:HEADER_LEN].hex()}")
    if journal[HEADER_LEN:] != expected_events(ticks):
        problems.append(f"journal events {journal[HEADER_LEN:].hex()}, expected {expected_events(ticks).hex()}")
    key = f"{calibration['rulesVersion']}/0x{level_id.hex()}/{weight}"
    if stored.get(key) != score:
        problems.append(f"stored bests {stored}, expected {key}: {score}")
    step_us, render_us, tick_us, overruns = map(int, re.findall(r"\d+", timing))
    if overruns != ticks - LATE_TICK:
        problems.append(f"{overruns} overruns, expected {ticks - LATE_TICK}: none before the clock's jump on tick "
                        f"{LATE_TICK}, then every tick")
    print(f"level {level} ({level_path.name}): end {end}, claim {claim} bp, ticks {ticks}, score {score}"
          + ("; native replay agrees" if not problems else ""))
    for name, us in (("step", step_us), ("render", render_us), ("tick", tick_us)):
        instructions = us * INSTRUCTIONS_PER_US
        print(f"  worst {name}: {us} us on qemu, {instructions / 1e6:.2f} M instructions, "
              f"{instructions / RP2040_MHZ / 1000:.1f} ms at {RP2040_MHZ} MHz and one instruction per cycle")
    return problems


def check(out, err, repo, irq_counts):
    lines = out.splitlines()
    calibration = json.loads((repo / "seasons" / "calibration-1.json").read_text())
    problems = []
    if "ScriptDone" not in err:
        problems.append("the app did not run to the end of the scripted inputs: " + err.strip()[-400:])
    locked = re.search(r"heap locked over (\d+) samples, (\d+) steps and (\d+) renders", err)
    printed = {re.match(r"jiro: heap (\w+):", line)[1] for line in lines if line.startswith("jiro: heap ")}
    if printed != set(STAGES):
        problems.append(f"the app printed the heap at stages {sorted(printed)}, expected {sorted(STAGES)}")
    outputs = level_outputs(lines)
    if len(outputs) != len(PLAYS):
        return problems + [f"the app ended {len(outputs)} levels, expected {len(PLAYS)}"]
    stores = [ast.literal_eval(line.split(" ", 2)[2]) for line in lines if line.startswith("save.store 'bests'")]
    stored = stores[-1] if stores else {}
    for level, output in zip(PLAYS, outputs):
        problems += check_level(repo, calibration, level, output, stored)
    ticks = sum(fields(line, "ticks")[0] for line, _, _ in outputs)
    if not locked or not int(locked[1]) == int(locked[2]) == ticks <= int(locked[3]):
        problems.append(f"the heap was locked over {locked and locked[0]}, expected every one of the {ticks} ticks")
    summaries = [line for line in lines if line.startswith("ui.page") and "'{g} next'" in line]
    if len(summaries) != len(PLAYS):
        problems.append(f"the app showed {len(summaries)} summary screens, expected {len(PLAYS)}")
    elif "best not saved: flash full" not in summaries[0] or "a new best" not in summaries[1]:
        problems.append(f"expected the first summary's best not saved, the flash full, then a new best: {summaries}")
    if irq_counts is not None and (not irq_counts or irq_counts[1] == 0 or irq_counts[2] != 0):
        problems.append(f"expected hard IRQs that interrupted jiro_core and none that failed, got (dispatched, in "
                        f"jiro_core, failed) {irq_counts or 'unread: the app did not end'}")
    return problems


def main(firmware, app, free_bytes, holes, hard_irq):
    repo = Path(__file__).resolve().parents[2]
    names = json.loads((app / "wedgie.json").read_text())["apps"][0]["files"]
    files = {name: (app / name).read_bytes() for name in names}
    stubs = dict(STUBS)
    script = [(held, tapped) for ticks, held, tapped in SCRIPT for _ in range(ticks)]
    stubs["lcd.py"] = (f"SQUEEZE_BYTES = {SQUEEZE_BYTES}\nWRAP_AFTER_US = {WRAP_AFTER_US}\nLATE_TICK = {LATE_TICK}\n"
                       f"LATE_US = {LATE_US}\nPLAY_COUNT = {len(PLAYS)}\nMENU = {menu_presses()!r}\nSCRIPT = {script!r}\n"
                       ) + stubs["lcd.py"]
    stubs["save.py"] = f"LFS_FILE_BYTES = {LFS_FILE_BYTES}\n" + stubs["save.py"]
    stubs["os_stub.py"] = f"NONCE = {RUN_NONCE!r}\n" + stubs["os_stub.py"]
    files.update({name: text.encode() for name, text in stubs.items()})
    sizes = [HOLE_SIZES[i % len(HOLE_SIZES)] for i in range(holes)]
    hole_bytes = sum((size + 15) // 16 * 16 + 16 for size in sizes)
    qemu = qemu_natmod_vectors.QEMU
    qemu[qemu.index("shift=0")] = f"shift={ICOUNT_SHIFT}"
    repl = qemu_natmod_vectors.RawRepl(firmware)
    try:
        # The placeholder, the filesystem's objects, the files, then the firmware's modules (the
        # stubs, with the 28,800-byte screen buffer), all before the ballast: on a wedgie the
        # files are in flash, and the free RAM it reports already excludes the firmware's modules.
        repl.exec(PLACEHOLDER.format(room=sum(map(len, files.values())) + UPLOAD_ROOM))
        repl.exec(f"_FILES = {{}}\nLFS_FILE_BYTES = {LFS_FILE_BYTES}\n" + VFS)
        upload_files(repl, files)
        repl.exec(PLACEHOLDER.format(room=STUB_ROOM))
        repl.exec("import lcd_buffer, sys, clock, os_stub; sys.modules['time'] = clock; sys.modules['os'] = os_stub")
        repl.exec("import lcd, ui, save, wedgie")
        if hard_irq:
            repl.exec(ARM_HARD_IRQ)
        print(repl.exec(BALLAST.format(free=free_bytes, holes=holes, hole_bytes=hole_bytes, hole_sizes=HOLE_SIZES,
                                       size_count=len(HOLE_SIZES))), end="")
        out, err = run_app(repl)
        irq_counts = None
        if hard_irq:
            irq_counts = ast.literal_eval(repl.exec("print(irqsim.stats())")) if "ScriptDone" in err else ()
    finally:
        repl.p.kill()
    print(out, end="")
    print(err.strip().splitlines()[-1] if err.strip() else "")
    if irq_counts:
        print(f"irqsim: {irq_counts[0]} hard IRQs, {irq_counts[1]} in jiro_core, {irq_counts[2]} failed")
    problems = check(out, err, repo, irq_counts)
    for problem in problems:
        print("FAIL:", problem)
    print("qemu play test:", "PASS" if not problems else f"FAIL ({len(problems)})")
    return 1 if problems else 0


if __name__ == "__main__":
    args = sys.argv[1:]
    hard_irq = args[-1:] == ["--hard-irq"]
    if hard_irq:
        args = args[:-1]
    if len(args) not in (3, 4):
        raise SystemExit(__doc__)
    sys.exit(main(args[0], Path(args[1]), int(args[2]), int(args[3]) if len(args) == 4 else HOLES, hard_irq))
