"""Fails unless a wedgie app folder keeps the wedgie's app limits (https://wedgie.dev/code.md, the
app format): wedgie.json lists at most 30 files of at most 256 KB in all, each present in the
folder, each landing in the flash's root under a name that is <mod>.py, <mod>.mpy or starts with
<mod>_, of a-z 0-9 _ ending .py .mpy .bin .json or .txt, and <mod>.py or <mod>.mpy is among them.
The mod is 2 to 20 of a-z 0-9 _ and starts with a letter, the name has at most 14 characters, the
about is one line of at most 140, the label, if any, is #rrggbb, and the icon, if any, 12 rows
of 12 of the characters . k w g r y b s.

Usage: python3 -I check_app_limits.py <app folder>
"""

import json
import re
import sys
from pathlib import Path

MAX_FILES = 30
MAX_BYTES = 256 * 1024
NAME = r"[a-z0-9_]*\.(py|mpy|bin|json|txt)"
MOD = r"[a-z][a-z0-9_]{1,19}"
MAX_NAME = 14  # it is printed on the cartridge
MAX_ABOUT = 140
LABEL = r"#[0-9a-fA-F]{6}"
ICON_ROW = r"[.kwgrybs]{12}"


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
        if not re.fullmatch(MOD, mod):
            found.append(f"{mod}: the mod is not 2 to 20 of a-z 0-9 _ starting with a letter")
        if not 1 <= len(app.get("name", "")) <= MAX_NAME:
            found.append(f"{mod}: the name {app.get('name')!r} is not 1 to {MAX_NAME} characters")
        about = app.get("about", "")
        if not 1 <= len(about) <= MAX_ABOUT or "\n" in about:
            found.append(f"{mod}: the about is not one line of 1 to {MAX_ABOUT} characters, but of {len(about)}")
        if "label" in app and not re.fullmatch(LABEL, app["label"]):
            found.append(f"{mod}: the label {app['label']!r} is not #rrggbb")
        icon = app.get("icon")
        if icon is not None and not (len(icon) == 12 and all(re.fullmatch(ICON_ROW, row) for row in icon)):
            found.append(f"{mod}: the icon is not 12 rows of 12 of the characters . k w g r y b s")
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
