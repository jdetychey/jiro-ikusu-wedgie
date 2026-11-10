#!/usr/bin/env bash
# Builds the Training app in wedgie/app/: jiro_app.mpy, precompiled with MicroPython v1.29.0's
# mpy-cross (scripts/build-qemu.sh builds it), the level file from levels/ and
# seasons/calibration-1.json, and a copy of jiro_core.mpy (make -C wedgie/natmod/glue builds it).
# Then checks the frames of the app's tick, and the wedgie's app limits on the app's folder and on
# the repository, whose wedgie.json is the one wedgie.dev reads.
set -euo pipefail

wedgie="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
micropython="${JIRO_WEDGIE_HOME:-$HOME/.cache/jiro-wedgie}/micropython"

"$micropython/mpy-cross/build/mpy-cross" -s jiro_app.py -o "$wedgie/app/jiro_app.mpy" "$wedgie/app/jiro_app.py"
python3 -I "$wedgie/scripts/check_app_frames.py" "$micropython/tools/mpy-tool.py" "$wedgie/app/jiro_app.mpy"
python3 -I "$wedgie/scripts/make_app_levels.py" "$wedgie/.." "$wedgie/app/jiro_levels.bin"
cp "$wedgie/natmod/glue/jiro_core.mpy" "$wedgie/app/jiro_core.mpy"
python3 -I "$wedgie/scripts/check_app_limits.py" "$wedgie/app"
python3 -I "$wedgie/scripts/check_app_limits.py" "$wedgie/.."
