"""Writes the check app's vector file from the game vectors of vectors/game/ and vectors/levels/
(README §6.6): what the app replays and the results it must reproduce. qemu_natmod_vectors.py
encodes its records with this module too, so that qemu and the wedgie check the same bytes with
the same code (wedgie/check/jiro_check.py, which reads them).

The file is MAGIC, the record count (u16), then one record per vector, integers big-endian:
name length (u8), name, weightBp (u16), error code (u8, 0 when replay accepts), end reason (u8, 0
on an error), tick count (u16), level length (u16), journal length (u16), the Outcome of README
§7.3 in field order (110 bytes, zero on an error), traceKeccak (32), the last tick's state hash
(32, zero with no tick), the level bytes, the journal bytes.

Usage: python3 -I make_check_vectors.py <repo> <out.bin>
"""

import json
import struct
import sys
from pathlib import Path

MAGIC = b"JV01"
KINDS = ("game", "levels")
# The vectors committed at M3.8. Vectors are only ever added (CLAUDE.md), so fewer means a broken
# path or checkout, which must not pass as an empty check.
MIN_VECTORS = 75
OUTCOME_LEN = 110
HASH_LEN = 32
OUTCOME_INTS = [("levelIndex", 1), ("drawSlot", 1), ("mode", 1), ("rulesVersion", 2), ("endReason", 1),
                ("claimBp", 2), ("ticksUsed", 2), ("levelScore", 4)]


def vector_names(repo):
    names = [f"{kind}/{path.stem}" for kind in KINDS for path in sorted((Path(repo) / "vectors" / kind).glob("*.json"))]
    if len(names) < MIN_VECTORS:
        raise SystemExit(f"{len(names)} vectors in {repo}/vectors/game and levels, expected at least {MIN_VECTORS}")
    return names


def hex_bytes(text):
    if not text.startswith("0x"):
        raise ValueError(f"not a 0x-prefixed byte string: {text[:20]}")
    return bytes.fromhex(text[2:])


def encode_outcome(outcome):
    fields = hex_bytes(outcome["journalHash"]) + hex_bytes(outcome["levelId"]) + hex_bytes(outcome["runNonce"])
    fields += b"".join(outcome[key].to_bytes(width, "big") for key, width in OUTCOME_INTS)
    if len(fields) != OUTCOME_LEN:
        raise ValueError(f"Outcome of {len(fields)} bytes, expected {OUTCOME_LEN}")
    return fields


def record(repo, name):
    path = Path(repo) / "vectors" / f"{name}.json"
    vector = json.loads(path.read_text())
    trace = (path.parent / vector["trace"]).read_bytes()
    if len(trace) % HASH_LEN:
        raise ValueError(f"{name}: trace of {len(trace)} bytes is not whole state hashes")
    level, journal = hex_bytes(vector["level"]), hex_bytes(vector["journal"])
    accepted = "outcome" in vector
    outcome = encode_outcome(vector["outcome"]) if accepted else bytes(OUTCOME_LEN)
    end_reason = vector["outcome"]["endReason"] if accepted else 0
    last_hash = trace[-HASH_LEN:] if trace else bytes(HASH_LEN)
    encoded_name = name.encode()
    return (bytes([len(encoded_name)]) + encoded_name
            + struct.pack(">HBBHHH", vector["weightBp"], vector.get("error", 0), end_reason,
                          len(trace) // HASH_LEN, len(level), len(journal))
            + outcome + hex_bytes(vector["traceKeccak"]) + last_hash + level + journal)


def main(repo, out):
    names = vector_names(repo)
    data = MAGIC + struct.pack(">H", len(names)) + b"".join(record(repo, name) for name in names)
    Path(out).write_bytes(data)
    print(f"{out}: {len(names)} vectors, {len(data)} bytes")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
