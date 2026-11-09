"""Fails unless a wedgie app folder keeps the wedgie's app limits (https://wedgie.dev/code.md, the
app format): wedgie.json lists at most 30 files of at most 256 KB in all, each present in the
folder, each landing in the flash's root under a name that is <mod>.py, <mod>.mpy or starts with
<mod>_, of a-z 0-9 _ ending .py .mpy .bin .json or .txt, and <mod>.py or <mod>.mpy is among them.

Usage: python3 -I check_app_limits.py <app folder>
"""

import json
import re
import sys
from pathlib import Path

MAX_FILES = 30
MAX_BYTES = 256 * 1024
NAME = r"[a-z0-9_]*\.(py|mpy|bin|json|txt)"


def problems(folder):
    found = []
    for app in json.loads((folder / "wedgie.json").read_text())["apps"]:
        mod, files = app["mod"], app["files"]
        total = 0
        for listed in files:
            path = folder / listed
            if not path.is_file():
                found.append(f"{mod}: {listed} is listed but missing")
                continue
            total += path.stat().st_size
            name = path.name
            if not re.fullmatch(NAME, name) or not (name in (f"{mod}.py", f"{mod}.mpy") or name.startswith(f"{mod}_")):
                found.append(f"{mod}: {name} does not start with {mod}_ or is not a-z 0-9 _ with a known extension")
        if not {f"{mod}.py", f"{mod}.mpy"} & {Path(listed).name for listed in files}:
            found.append(f"{mod}: neither {mod}.py nor {mod}.mpy is listed")
        if len(files) > MAX_FILES:
            found.append(f"{mod}: {len(files)} files, more than {MAX_FILES}")
        if total > MAX_BYTES:
            found.append(f"{mod}: {total} bytes, more than {MAX_BYTES}")
        print(f"app {mod}: {len(files)} files of {MAX_FILES}, {total} bytes of {MAX_BYTES}")
    return found


def main(folder):
    found = problems(Path(folder))
    for problem in found:
        print(problem)
    print("app limits: " + ("PASS" if not found else f"FAIL ({len(found)})"))
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
