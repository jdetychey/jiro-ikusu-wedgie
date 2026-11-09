"""Writes the Training app's level file from the 16 levels of levels/ and the scoring of
seasons/calibration-1.json (README §8.1, §8.4): what the app plays and how it scores it.

The calibration lists the level ids in slot order. Python has no keccak256, so this script pairs
them with the levels by the level_id column of levels/README.md, which jiro-tools generates from
the same files; the app then checks each level's id against the one the core computes when it
starts the level.

The file is MAGIC, the rules version (u16), the level count (u8), then one record per level in
slot order, integers big-endian: name length (u8), name, weightBp (u16), level id (32), level
bytes (472).

Usage: python3 -I make_app_levels.py <repo> <out.bin>
"""

import json
import re
import struct
import sys
from pathlib import Path

MAGIC = b"JL01"
LEVEL_LEN = 472
SEASON_LEVELS = 16


def table_ids(repo):
    """Level file stem -> level id, from the levels table of levels/README.md."""
    rows = re.findall(r"^\| (\d\d) \| (\w+) \| `(0x[0-9a-f]{64})` \|", (repo / "levels" / "README.md").read_text(), re.M)
    return {f"{number}-{name}": level_id for number, name, level_id in rows}


def records(repo):
    calibration = json.loads((repo / "seasons" / "calibration-1.json").read_text())
    ids, weights = calibration["levelIds"], calibration["weightsBp"]
    paths = sorted((repo / "levels").glob("[0-9][0-9]-*.bin"))
    if not len(paths) == len(ids) == len(weights) == SEASON_LEVELS:
        raise SystemExit(f"{len(paths)} levels, {len(ids)} ids and {len(weights)} weights; expected {SEASON_LEVELS} of each")
    by_stem = table_ids(repo)
    for path, level_id, weight in zip(paths, ids, weights):
        if by_stem.get(path.stem) != level_id:
            raise SystemExit(f"{path.name}: levels/README.md gives {by_stem.get(path.stem)}, the calibration {level_id}")
        level = path.read_bytes()
        if len(level) != LEVEL_LEN or not 1 <= weight <= 0xFFFF:
            raise SystemExit(f"{path.name}: {len(level)} bytes, weight {weight}")
        name = path.stem.split("-", 1)[1].title().encode()
        yield bytes([len(name)]) + name + struct.pack(">H", weight) + bytes.fromhex(level_id[2:]) + level


def main(repo, out):
    repo = Path(repo)
    rules_version = json.loads((repo / "seasons" / "calibration-1.json").read_text())["rulesVersion"]
    data = MAGIC + struct.pack(">HB", rules_version, SEASON_LEVELS) + b"".join(records(repo))
    Path(out).write_bytes(data)
    print(f"{out}: {SEASON_LEVELS} levels, rules version {rules_version}, {len(data)} bytes")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
