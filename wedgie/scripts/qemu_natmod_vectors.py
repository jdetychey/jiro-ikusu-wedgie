"""Loads jiro_core.mpy into MicroPython v1.29.0's qemu MICROBIT port (Cortex-M0, an 8 KB VM stack
limit; scripts/build-qemu.sh builds it) and checks game vectors in one MicroPython session, with the
check app's own code (wedgie/check/jiro_check.py) on records of the app's own encoder
(make_check_vectors.py): replay's code, the Outcome, the tick count, traceKeccak and the last
state hash, then, for a vector replay accepts, the Recorder's end reason, journal and final state
hash. For a vector that fails, it prints the first tick whose state hash differs from the
committed trace. It prints the stack peak of start, step and replay on one 48 KB stack: start's
and step's must stay under the Training app's 24 KB stack, replay's under the check app's 48 KB,
the stacks the glue requires for each call. Usage:

  python3 -I qemu_natmod_vectors.py <firmware.elf> <jiro_core.mpy> <repo> (--all | <kind/name>...)

where --all is every vector of vectors/game/ and vectors/levels/. It stops qemu when it ends,
but hangs if the module hard-faults: run it under a timeout. Exit status 0 when every vector
passes and every peak stays under its stack.
"""

import ast
import base64
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

spec = importlib.util.spec_from_file_location("make_check_vectors", Path(__file__).parent / "make_check_vectors.py")
make_check_vectors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_check_vectors)

STACK_BYTES = 49152  # the check app's, which runs replay
CALL_STACK_BYTES = {"start": 24576, "step": 24576, "replay": STACK_BYTES}
DIAGNOSED_TICKS = 1024
QEMU = ["qemu-system-arm", "-M", "microbit", "-global", "nrf51-soc.flash-size=1048576",
        "-global", "nrf51-soc.sram-size=262144", "-nographic", "-monitor", "null",
        "-serial", "stdio", "-semihosting", "-icount", "shift=0", "-kernel"]

# A RAM filesystem holding one file, as MicroPython's tests/run-natmodtests.py injects an .mpy.
VFS = """\
import sys, io, vfs
class _F(io.IOBase):
  def __init__(s): s.o = 0
  def ioctl(s, r, a): return 0 if r == 4 else -1
  def readinto(s, b):
    b[:] = memoryview(_MPY)[s.o:s.o + len(b)]; s.o += len(b); return len(b)
class _FS:
  def mount(s, r, m): pass
  def chdir(s, p): pass
  def stat(s, p):
    if p == '/jiro_core.mpy': return tuple(0 for _ in range(10))
    raise OSError(-2)
  def open(s, p, m): return _F()
vfs.mount(_FS(), '/ram'); sys.path.insert(0, '/ram')
"""

# Stands in for the module in check(): it fills the stack with a pattern before a call and
# finds afterwards the lowest byte the call changed. check() calls start, then step once per
# tick, then journal_equals, so the steps' peak is read there, after start's peak was read and the
# stack filled again. replay_us times replay alone, without the fill.
PROBE = f"""\
import io, time
_P = b'\\xa5' * 1024
peaks = {{'start': 0, 'step': 0, 'replay': 0}}
def _fill():
  for i in range(0, {STACK_BYTES}, 1024): stack[i:i + 1024] = _P
def _note(name):
  i = 0
  while i < {STACK_BYTES} and stack[i:i + 1024] == _P: i += 1024
  while i < {STACK_BYTES} and stack[i] == 0xa5: i += 1
  peaks[name] = max(peaks[name], {STACK_BYTES} - i)
class _Probe:
  def replay(s, *a):
    global replay_us
    _fill(); t = time.ticks_us(); r = jc.replay(*a); replay_us = time.ticks_diff(time.ticks_us(), t)
    _note('replay'); return r
  def start(s, *a):
    _fill(); r = jc.start(*a); _note('start'); _fill(); return r
  def step(s, *a):
    return jc.step(*a)
  def journal_equals(s, *a):
    _note('step'); return jc.journal_equals(*a)
  def state_hash(s, *a):
    return jc.state_hash(*a)
probe = _Probe()
"""


class RawRepl:
    def __init__(self, firmware):
        self.p = subprocess.Popen(QEMU + [firmware], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.read_until(b">>> ")
        self.p.stdin.write(b"\r\x01")
        self.p.stdin.flush()
        self.read_until(b"raw REPL; CTRL-B to exit\r\n>")

    def read_until(self, end):
        data = b""
        while not data.endswith(end):
            byte = self.p.stdout.read(1)
            if not byte:
                raise SystemExit(f"qemu exited; output so far: {data[-400:]!r}")
            data += byte
        return data[: -len(end)]

    def exec(self, code):
        self.p.stdin.write(code.encode() + b"\x04")
        self.p.stdin.flush()
        if self.read_until(b"OK") != b"":
            raise SystemExit("raw REPL did not acknowledge the code")
        out, err = self.read_until(b"\x04"), self.read_until(b"\x04")
        self.read_until(b">")
        if err:
            raise SystemExit(f"device error: {err.decode()}")
        return out.decode()

    def upload(self, name, data, chunk=3000):
        self.exec(f"{name} = bytearray({len(data)})")
        for off in range(0, len(data), chunk):
            b64 = base64.b64encode(data[off:off + chunk]).decode()
            self.exec(f"import binascii; {name}[{off}:{off + min(chunk, len(data) - off)}] = binascii.a2b_base64('{b64}')")


def first_differing_tick(repl, repo, name):
    """Replays the vector again with a trace buffer, and compares it with the committed trace
    state hash by state hash (README §6.6, Diagnosis). The buffer holds at most DIAGNOSED_TICKS
    hashes: a whole trace (86,400 bytes) does not fit beside the stack in the qemu port's heap."""
    path = Path(repo) / "vectors" / f"{name}.json"
    expected = (path.parent / json.loads(path.read_text())["trace"]).read_bytes()[:32 * DIAGNOSED_TICKS]
    if not expected:
        return "the committed trace is empty"
    repl.upload("R", make_check_vectors.record(repo, name))
    got = bytes.fromhex("".join(repl.exec(
        f"case = read_case(io.BytesIO(R)); del R; T = bytearray({len(expected)}); jc.replay(case[8], case[9], case[1], stack, T)\n"
        "for i in range(0, len(T), 1024): print(T[i:i + 1024].hex())\n"
        "del case, T; gc.collect()").split()))
    for tick in range(len(expected) // 32):
        if got[32 * tick:32 * tick + 32] != expected[32 * tick:32 * tick + 32]:
            return f"first differing tick: {tick}"
    return f"ticks 0 to {len(expected) // 32 - 1} match the committed trace"


def main(firmware, mpy, repo, names):
    repl = RawRepl(firmware)
    try:
        repl.upload("_MPY", Path(mpy).read_bytes())
        repl.exec(VFS)
        print(repl.exec(f"import gc; gc.collect(); f0 = gc.mem_free(); stack = bytearray({STACK_BYTES}); "
                        "import jiro_core as jc; rec = bytearray(jc.recorder_size()); gc.collect(); "
                        f"print('rules', jc.rules_version(), 'recorder', len(rec), 'module bytes', "
                        f"f0 - gc.mem_free() - {STACK_BYTES} - len(rec))"), end="")
        repl.exec((Path(repo) / "wedgie" / "check" / "jiro_check.py").read_text())
        repl.exec(PROBE)
        failed = accepted = 0
        for name in names:
            repl.upload("R", make_check_vectors.record(repo, name))
            out = repl.exec("case = read_case(io.BytesIO(R)); failures, _ = check(probe, stack, rec, case); "
                            "print(case[2], case[4], replay_us, failures); del case, R; gc.collect()")
            code, ticks, us, failures = out.split(" ", 3)
            bad = failures.strip() != "[]"
            failed += bad
            accepted += code == "0"
            print(("FAIL " if bad else "ok   ") + f"{name}: code {code}, ticks {ticks}, replay {us} us (icount)"
                  + (f" {failures.strip()}" if bad else ""))
            if bad:
                print(f"     {first_differing_tick(repl, repo, name)}")
        peaks = ast.literal_eval(repl.exec("print(peaks)"))
        print(f"stack peaks of {STACK_BYTES} bytes: start {peaks['start']}, step {peaks['step']}, replay {peaks['replay']}")
        over = [call for call, peak in peaks.items() if peak >= CALL_STACK_BYTES[call]]
        if over:
            print(f"FAIL: {', '.join(f'{call} reached {CALL_STACK_BYTES[call]} bytes' for call in over)}")
        # A peak of 0 means the probe saw no call: the run checked nothing.
        unmeasured = [call for call, peak in peaks.items() if peak == 0 and (call == "replay" or accepted)]
        if unmeasured:
            print(f"FAIL: no stack use measured for {', '.join(unmeasured)}")
        print(f"vectors: {len(names)}, failed: {failed}")
        return 1 if failed or over or unmeasured else 0
    finally:
        repl.p.kill()


if __name__ == "__main__":
    if len(sys.argv) < 5:
        raise SystemExit(__doc__)
    repo = sys.argv[3]
    names = make_check_vectors.vector_names(repo) if sys.argv[4:] == ["--all"] else sys.argv[4:]
    sys.exit(main(sys.argv[1], sys.argv[2], repo, names))
