# The Jiro core check's code, which jiro.py runs once it holds the core's buffers. Shipped
# precompiled as jiro_check.mpy (wedgie/scripts/build-check.sh), so that loading it builds no
# parse tree on the wedgie's heap. wedgie/scripts/qemu_natmod_vectors.py runs read_case and check
# on MicroPython's qemu port, on the records of the same encoder (make_check_vectors.py).
import gc
import struct
import time

MAGIC = b"JV01"
VECTORS = "jiro_vectors.bin"
PAGE_LINES = 8
NAME_CHARS = 20  # "FAIL 75 " and the name's end fill a line of ui.COLS_SMALL (28)


def read_case(f):
    name = f.read(f.read(1)[0]).decode()
    weight, code, end_reason, ticks, level_len, journal_len = struct.unpack(">HBBHHH", f.read(10))
    outcome, keccak, last = f.read(110), f.read(32), f.read(32)
    return name, weight, code, end_reason, ticks, outcome, keccak, last, f.read(level_len), f.read(journal_len)


def check(jc, stack, recorder, case):
    """Replays a case and, if replay accepts it, plays its journal's inputs through the Recorder.
    Returns the names of what differs and replay's time in microseconds."""
    name, weight, code, end_reason, ticks, outcome, keccak, last, level, journal = case
    start = time.ticks_us()
    got_code, got_outcome, got_ticks, got_keccak, got_last = jc.replay(level, journal, weight, stack)
    replay_us = time.ticks_diff(time.ticks_us(), start)
    failures = [field for field, ok in (
        ("code", got_code == code),
        ("outcome", got_outcome == outcome),
        ("ticks", got_ticks == ticks),
        ("traceKeccak", got_keccak == keccak),
        ("last hash", got_last == last),
    ) if not ok]
    if code == 0:
        failures += recorder_failures(jc, stack, recorder, ticks, end_reason, last, level, journal)
    return failures, replay_us


def recorder_failures(jc, stack, recorder, ticks, end_reason, last, level, journal):
    started = jc.start(recorder, level, journal[3], journal[4:36], journal[68], stack)
    if started != 0:
        return ["recorder start %d" % started]
    # The journal's input events (tick: 2 bytes, mask: 1 byte) give the mask held at each tick.
    event, events_end = 71, 71 + 3 * (journal[69] << 8 | journal[70])
    mask = end = 0
    for tick in range(ticks):
        if event < events_end and journal[event] << 8 | journal[event + 1] == tick:
            mask = journal[event + 2]
            event += 3
        end = jc.step(recorder, mask, stack)
        if end:
            break
    failures = []
    if end != end_reason:
        failures.append("recorder end %d" % end)
    if not jc.journal_equals(recorder, journal):
        failures.append("recorder journal")
    if jc.state_hash(recorder, stack) != last:
        failures.append("recorder hash")
    return failures


def check_all(stack, recorder, free_before, screen):
    import jiro_core as jc
    import ui
    if jc.recorder_size() > len(recorder):
        raise ValueError("jiro_core needs a %d B Recorder" % jc.recorder_size())
    gc.collect()
    print("jiro_core rules version", jc.rules_version(), "free before", free_before, "after", gc.mem_free())
    lines, failed = [], 0
    started = time.ticks_ms()
    with open(VECTORS, "rb") as f:
        if f.read(4) != MAGIC:
            raise ValueError(VECTORS + " is not a JV01 vector file")
        count = struct.unpack(">H", f.read(2))[0]
        for index in range(count):
            # A screen that stays on the boot logo for minutes looks like a frozen wedgie.
            ui.page(screen, "Jiro check", [("vector %d of %d" % (index + 1, count), ui.INK),
                                           ("about 4 min in all", ui.MUTED)], "")
            # One case in RAM at a time.
            case = read_case(f)
            failures, replay_us = check(jc, stack, recorder, case)
            failed += bool(failures)
            verdict = "FAIL" if failures else "ok"
            print(verdict, "%d/%d" % (index + 1, count), case[0], "ticks", case[4], "replay", replay_us, "us", failures)
            lines.append("%s %d %s" % (verdict, index + 1, case[0].split("/")[1][-NAME_CHARS:]))
            del case
            gc.collect()
    print("jiro core check:", "PASS" if failed == 0 else "FAIL %d of %d" % (failed, count),
          "in", time.ticks_diff(time.ticks_ms(), started) // 1000, "s")
    return lines, failed


def show(lines, failed, screen):
    import lcd
    import ui
    title = "Jiro PASS" if failed == 0 else "Jiro FAIL %d" % failed
    pages = (len(lines) + PAGE_LINES - 1) // PAGE_LINES
    keys, page = lcd.Keys(), 0
    while True:
        shown = lines[page * PAGE_LINES:(page + 1) * PAGE_LINES]
        ui.page(screen, "%s %d/%d" % (title, page + 1, pages),
                [(line, ui.GREEN_D if line.startswith("ok") else ui.RED) for line in shown], "{g} next")
        while "A" not in keys.pressed():
            time.sleep_ms(50)
        page = (page + 1) % pages
