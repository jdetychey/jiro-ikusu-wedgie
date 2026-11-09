"""Fails unless a .mpy file starts with the header of an armv6m native module of MicroPython
v1.29.0, the header of the wedgie firmware's own native modules (firmware 0.3.76): b"M", mpy
version 6, then (native arch armv6m = 4) << 2 | mpy sub-version 3, then 31 small-int bits.

Usage: python3 -I check_mpy_header.py <module.mpy>
"""

import sys

EXPECTED = bytes([ord("M"), 6, 4 << 2 | 3, 31])


def main(path):
    with open(path, "rb") as module:
        header = module.read(len(EXPECTED))
    verdict = "PASS" if header == EXPECTED else f"FAIL, expected {EXPECTED.hex(' ')}"
    print(f"{path}: header {header.hex(' ')} (mpy v6.3, armv6m, 31-bit small ints): {verdict}")
    return 0 if header == EXPECTED else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
