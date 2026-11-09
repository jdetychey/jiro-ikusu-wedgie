#!/usr/bin/env bash
# Builds the check app in wedgie/check/: jiro_check.mpy, precompiled with MicroPython v1.29.0's
# mpy-cross (scripts/build-qemu.sh builds it), the vector file from vectors/, and a copy of
# jiro_core.mpy (make -C wedgie/natmod/glue builds it). Then checks the wedgie's app limits.
set -euo pipefail

wedgie="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mpy_cross="${JIRO_WEDGIE_HOME:-$HOME/.cache/jiro-wedgie}/micropython/mpy-cross/build/mpy-cross"

"$mpy_cross" -s jiro_check.py -o "$wedgie/check/jiro_check.mpy" "$wedgie/check/jiro_check.py"
python3 -I "$wedgie/scripts/make_check_vectors.py" "$wedgie/.." "$wedgie/check/jiro_vectors.bin"
cp "$wedgie/natmod/glue/jiro_core.mpy" "$wedgie/check/jiro_core.mpy"
python3 -I "$wedgie/scripts/check_app_limits.py" "$wedgie/check"
