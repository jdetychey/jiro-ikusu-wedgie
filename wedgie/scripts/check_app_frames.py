"""Fails unless the Training app's tick keeps its Python frames off the heap, but the one of its
drawing. MicroPython v1.29.0 takes a call's frame from the heap when its state is over 44 bytes,
4 for each local and stack slot and 12 for each exception handler (py/objfun.c,
VM_MAX_STATE_ON_STACK): a first-fit search, which a fragmented heap makes slow, and slower the
larger the frame. So the functions a tick calls stay within 44 bytes, and the drawing, which does
not, is one call (wedgie/README.md, the tick). The play test cannot see a frame that outgrew
this: with the heap locked, MicroPython puts such a frame on the C stack instead.

Reads the frames from the disassembly of MicroPython's tools/mpy-tool.py, which lists every
scope's state slots and exception handlers, each scope followed by its children.

Usage: python3 -I check_app_frames.py <micropython>/tools/mpy-tool.py <jiro_app.mpy>
"""

import re
import subprocess
import sys

STATE_ON_C_STACK = 44  # bytes: VM_MAX_STATE_ON_STACK on a 32-bit port
SLOT_BYTES, HANDLER_BYTES, HEADER_BYTES, HEAP_BLOCK = 4, 12, 20, 16  # mp_obj_t, mp_exc_stack_t, mp_code_state_t
# What Training.ticks calls on every tick, besides jiro_core and the firmware.
ON_C_STACK = ("sample", "wait_until")
# The tick's one frame on the heap, and the heap blocks it may take.
ON_HEAP = {"Training.render": 7}


def frames(disassembly):
    """Qualified name -> (state slots, exception handlers) of every scope of a module."""
    scopes = []
    for line in disassembly.splitlines():
        if line.startswith("simple_name: "):
            scopes.append([line.split(": ", 1)[1], None, 0])
        elif line.startswith("  prelude: ("):
            scopes[-1][1] = tuple(map(int, re.findall(r"\d+", line)[:2]))
        elif line.startswith("  children: ["):
            names = line.split("[", 1)[1].rstrip("]")
            scopes[-1][2] = len(names.split(", ")) if names else 0
    found = {}

    def walk(at, prefix):
        name, prelude, children = scopes[at]
        found[prefix + name] = prelude
        at += 1
        for _ in range(children):
            at = walk(at, "" if name == "<module>" else prefix + name + ".")
        return at

    if not scopes or walk(0, "") != len(scopes):
        raise SystemExit("mpy-tool's disassembly did not list every scope with its children")
    return found


def main(mpy_tool, mpy):
    disassembly = subprocess.run([sys.executable, mpy_tool, "-d", mpy], capture_output=True, text=True, check=True)
    found = frames(disassembly.stdout)
    problems = []
    for name in ON_C_STACK + tuple(ON_HEAP):
        if found.get(name) is None:
            problems.append(f"{name}: not in {mpy}")
            continue
        slots, handlers = found[name]
        state = slots * SLOT_BYTES + handlers * HANDLER_BYTES
        blocks = -(-(HEADER_BYTES + state) // HEAP_BLOCK)
        if name in ON_C_STACK:
            print(f"{name}: {state} bytes of state, of {STATE_ON_C_STACK} on the C stack")
            if state > STATE_ON_C_STACK:
                problems.append(f"{name}: {state} bytes of state, over {STATE_ON_C_STACK}: every tick would take its "
                                "frame from the heap")
        else:
            print(f"{name}: {state} bytes of state, a frame of {blocks} heap blocks, of {ON_HEAP[name]}")
            if state <= STATE_ON_C_STACK:
                problems.append(f"{name}: its frame now fits the C stack: move it to ON_C_STACK")
            elif blocks > ON_HEAP[name]:
                problems.append(f"{name}: a frame of {blocks} heap blocks, over {ON_HEAP[name]}")
    for problem in problems:
        print(problem)
    print("app frames: " + ("PASS" if not problems else f"FAIL ({len(problems)})"))
    return 1 if problems else 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    sys.exit(main(sys.argv[1], sys.argv[2]))
