# Vectors

Golden vectors that every target of the core must reproduce (README §6.6). They are generated;
never edit them by hand. CI regenerates them and fails on any difference (`scripts/check-drift.sh vectors/`), and on
moved vectors without a version bump (`scripts/check-rules-version.sh`, below).

| Path | Content | Written by |
|---|---|---|
| `format/journal-*.json` | Journal bytes v1 (README §6.3): valid journals, and one malformed journal per decoder rule | `cargo run -p jiro-tools --locked -- vectors` |
| `format/level-*.json` | Level bytes v1 (README §5): valid levels, one invalid level per range and validity rule, and the §5 worked examples | `cargo run -p jiro-tools --locked -- vectors` |
| `game/*.json`, `game/*.trace` | Game vectors (README §6.6): a level and a journal, with the Outcome or error that replay returns, and the trace of state hashes v1 | `cargo run -p jiro-tools --locked -- vectors` |
| `levels/*.json`, `levels/*.trace` | The gate set of the M4 gas gate (plan §4), game vectors with no `covers`: for each season level, `NN-worst`, its longest bot journal, and `NN-synthetic`, the longest run of the worst-case scenario's policy, scored with the level's weight | `cargo run -p jiro-tools --release --locked -- calibrate --check --out seasons/calibration-1.json` |
| `COVERAGE.md` | The coverage index: every rule tag of README §4 and of the `parity-testing` skill, with the game vectors that cover it and the core tests that name it | `cargo run -p jiro-tools --locked -- vectors` |
| `derivations.json` | Level seeds and deflection keys, level ids, boss starts, draws and run commitments (README §6.4, §8.2), and settlement deadlines (README §7.2), computed with ethers | `node tools/xcheck/derivations.mjs --write` |

Kinds and fields:

- **`journal`:** `kind`, `name`, `input`, then `reencoded` (the decoded journal encoded again, equal to `input`) or `error` (the code of README §6.3).
- **`level`:** `kind`, `name`, `input`, then `levelId` and `reencoded`, or `error` (the code of README §5).
- **`game`:** `kind`, `name`, `covers` (README sections and rule tags), the replay inputs `rulesVersion`, `level`, `runNonce`, `levelIndex`, `mode`, `weightBp` and `journal`, then `outcome` (the eleven fields of README §7.3) or `error` (the code of README §6.3), then `trace` and `traceKeccak`. A trace holds the 32-byte state hash of every processed tick, in tick order.

Byte strings are lowercase hex with a `0x` prefix, and integers are JSON numbers.

The scenarios live in `tools/src/scenarios/`. Each states what the core must return for its bytes, and generation stops if the core disagrees. The game scenarios play their journals through the Recorder, scripted or with policies that read the game, and check that the run does what the vector is named for: a scenario that stops exercising its rule stops generation. `core/tests/vectors.rs` and `core/tests/derivations.rs` check the core against every file here, and `core/tests/coverage_index.rs` checks `COVERAGE.md` against README.md, the skill, the vectors and the core tests.

**Named game vectors** that later items use: `open-field-empty` (the README §6.4 worked example), `open-field-capture` (captures on the open-field level, then a commit) and `worst-case` (2,700 ticks with an event on every tick, a B = 8 boss and 3 minions moving every tick, obstacles and frequent captures: the gas and cycle benchmark).

**Version gate.** `scripts/check-rules-version.sh` compares the tree with the merge base of `origin/main`. A modified or deleted game vector requires a new `RULES_VERSION`, recorded by every game vector; a modified or deleted format vector requires a new format version (`JOURNAL_VERSION`, `LEVEL_VERSION`, `STATE_HASH_VERSION`) or `RULES_VERSION`. Added vectors pass. `--reverse` fails on any moved vector or changed version.

**Diagnosis.** When a target disagrees with a vector, `core/tests/vectors.rs` (or the target's runner) names the first differing tick. Then:

```bash
cargo run -p jiro-tools --locked -- replay --vector vectors/game/<name>.json --dump-tick <t> --diff <the other target's preimage at t>
```

prints the Outcome, the native preimage at tick t field by field, the fields (and grid cells) that differ, and the final state hash on its own line. `--level <file> --journal <file>` replays a level and a journal file instead, and `--vector <file> --journal <file>` replaces the vector's journal.

To regenerate after a change that is meant to move them (see the sync table in `CLAUDE.md`):

```bash
cargo run -p jiro-tools --locked -- vectors
npm --prefix tools/xcheck ci && node tools/xcheck/derivations.mjs --write
```
