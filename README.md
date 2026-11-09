# Jiro-ikusu for the wedgie

Jiro-ikusu is a territory-capture arcade game: you cut lines across the field to claim it, while a boss and its minions roam the part that's still open. Claim enough of the field and the level is cleared.

This is the [wedgie](https://wedgie.dev) app. It plays the game's 16 levels in Training mode, offline, at 15 ticks a second, and keeps your best score for each level on the wedgie. The same levels play in a browser at [jiro-ikusu.win](https://jiro-ikusu.win).

## Controls

| Key | In a level |
|---|---|
| Joystick | Move. Off the border you cut a trail; closing it on claimed ground captures the side the boss isn't on. |
| A (green) | Fast cut: two cells a tick while you cut |
| B (grey) | Slow shield: one cell every second tick; it absorbs one hit |
| X (grey) | Commit: end the level now and keep its claim |

On the level select, the joystick picks a level and green plays it. After a level, green goes on, to the next level after a clear. After the last level, a screen points to [jiro-ikusu.win](https://jiro-ikusu.win), and green starts again at level 1. Y (red) and the joystick press are not used.

## Saves

The app keeps one best score per level in the save `bests`, under `/saves/jiro/`. Installing an app, this one or another, never touches it.

## Install

With wedgie's command-line tool, [wedgie.py](https://wedgie.dev/wedgie.py), from the app's folder:

```bash
cd wedgie/app
python3 wedgie.py install .
```

Then press green on the wedgie. Installed this way, from a repository rather than from wedgie.dev's shelf, the app is not signed by wedgie.dev and installs with full access. This one only draws, reads the keys and keeps its save. If a wedgie hangs on an app, unplug it, hold both grey buttons (B and X) while plugging it back in, which starts it without its app, then run `python3 wedgie.py uninstall`.

## What is in this repository

| Path | What |
|---|---|
| `wedgie.json` | The app's manifest for wedgie.dev, at the root, with the files of `wedgie/app/` |
| `wedgie/app/` | The app as a wedgie installs it: `jiro.py`, `jiro_app.mpy` (compiled from `jiro_app.py`), `jiro_core.mpy` (the game's rules) and `jiro_levels.bin` (the 16 levels and their score weights), with its own `wedgie.json` |
| `core/` | jiro-core, the game's rules in Rust: the same code decides every game in the browser and onchain |
| `shell/` | jiro-shell: which cells changed since the last frame |
| `wedgie/natmod/` | `jiro_core.mpy`, jiro-core as a MicroPython native module: a Rust crate for the RP2040's Cortex-M0 and its C glue |
| `wedgie/scripts/` | The build, and the checks on MicroPython's qemu port: every vector through the module, the app's play test, the app limits |
| `wedgie/check/` | The check app, which replays every vector through the module on a wedgie |
| `levels/`, `seasons/calibration-1.json`, `vectors/` | The 16 levels, their score weights, and the golden vectors the module is checked against |
| `tools/` | The `replay` command the play test checks the app's journals with |
| `wedgie/README.md`, `reports/wedgie-2026-11-02.md` | How the app and the module work, their RAM and stack budget, and the record of the rounds on a wedgie |

## Build it

Linux, with `rustup` and `apt` (the CI runs Ubuntu 24.04):

```bash
bash scripts/bootstrap.sh            # Rust 1.98.0, gcc-arm-none-eabi, qemu-system-arm, MicroPython v1.29.0, pyelftools
make -C wedgie/natmod/glue           # jiro_core.mpy
bash wedgie/scripts/build-app.sh     # wedgie/app/: jiro_app.mpy, jiro_core.mpy, jiro_levels.bin, and the app limits
git status --short wedgie/app        # nothing: the committed files are what the sources build
```

The build is reproducible: the module's bytes do not depend on where the repository is checked out, and the CI fails if the files committed in `wedgie/app/` are not what the commit builds.

## Checks

The CI (`.github/workflows/ci.yml`) runs:

- the tests of jiro-core and jiro-shell;
- every game and level vector through `jiro_core.mpy` on MicroPython's qemu port for a Cortex-M0, with the stack each call uses;
- the app's play test on qemu: two levels with scripted keys, the heap filled down to the 137,760 bytes a wedgie has free, every journal checked against native replay. It runs plain and with a hard interrupt at every tick, as the wedgie firmware's key interrupt.

`wedgie/README.md` gives the commands and what each check covers.

## Memory and timing

The app needs about 62 KB of RAM in three blocks: a 24 KB stack for the core, the core's 18.5 KB Recorder, and the module's 18.6 KB. It allocates them first, while the heap is in one piece. A power-up also starts the wedgie's USB drive, which leaves about 113.6 KB free; on a heap like that the app keeps about 33 KB free at the level select, and about 58 KB without the drive. A tick's game step takes under 10 ms on a wedgie. A capture of most of the field takes longer to draw, about one tick, and the app pushes it as full-width rows, the screen's fast path.

## Where it comes from

This repository is the wedgie part of Jiro-ikusu, whose main repository is private for now. Its files are that repository's at the commit of its pull request for wedgie.dev's shelf. These differ:

- this README, `wedgie.json` at the root, `.github/workflows/ci.yml`, `scripts/bootstrap.sh` (its wedgie tier only) and `Cargo.toml` at the root;
- `tools/`, cut down to the `replay` command;
- `core/tests/`, without the two suites that need the main repository: its property tests, and its index of the specification's rules;
- `wedgie/natmod/rust/`, a member of the root workspace, which makes the module's bytes the same in every checkout.

`wedgie/README.md` and the report are copied as they are. Their references to `README.md`, `IMPLEMENTATION_PLAN.md` and `docs/` point into the main repository.

## License

MIT, in `LICENSE`.
