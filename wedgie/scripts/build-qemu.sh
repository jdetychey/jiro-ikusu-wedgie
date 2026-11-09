#!/usr/bin/env bash
# Builds MicroPython v1.29.0's mpy-cross and its qemu port for the MICROBIT board (nRF51,
# Cortex-M0, armv6m) in the clone that `scripts/bootstrap.sh wedgie` makes: native .mpy loading
# enabled (MICROPY_EMIT_THUMB=1) as on the rp2 port, the VM's stack limit at 8 KB as the rp2
# port's PICO_STACK_SIZE=0x2000 (a soft limit the VM checks, not a physical stack; the core never
# runs on the C stack, since call_on_stack moves it to the app's 48 KB buffer, and the wedgie run
# is the real check), a 236 KB heap, and no frozen manifest, which would need the micropython-lib
# submodule. The board's QEMU_ARGS give the machine 256 KB of RAM: the heap leaves 19 KB for the C
# stack and 800 bytes of other statics, and holds the app's play test (qemu_app_check.py): its
# files, the 28,800-byte screen buffer and the 137,760 bytes a wedgie has free. Prints the
# firmware's path.
#
# Usage: build-qemu.sh [irqsim]
# irqsim builds the firmware, in build-MICROBIT-irqsim, with the irqsim user C module (irqsim/):
# the port's SysTick interrupt dispatches a hard IRQ callback, as the wedgie firmware's key IRQ
# does, for qemu_app_check.py --hard-irq.
set -euo pipefail

case "$*" in
  "" | irqsim) ;;
  *) echo "usage: build-qemu.sh [irqsim]" >&2; exit 2 ;;
esac

mpy_dir="${JIRO_WEDGIE_HOME:-$HOME/.cache/jiro-wedgie}/micropython"
log="$mpy_dir/build-qemu.log"

build() {
  if ! "$@" >>"$log" 2>&1; then
    tail -n 30 "$log" >&2
    echo "build-qemu: failed: $*" >&2
    exit 1
  fi
}

: >"$log"
build make -C "$mpy_dir/mpy-cross" -j"$(nproc)"
port_flags=(BOARD=MICROBIT FROZEN_MANIFEST= MICROPY_HEAP_SIZE=241664 MICROPY_STACK_SIZE=8192
  CFLAGS_EXTRA="-DMICROPY_EMIT_THUMB=1 -DMICROPY_EMIT_INLINE_THUMB=1")
build_name=build-MICROBIT
if [ "$#" -eq 1 ]; then
  build_name=build-MICROBIT-irqsim
  port_flags+=(BUILD="$build_name" USER_C_MODULES="$(cd "$(dirname "${BASH_SOURCE[0]}")/irqsim" && pwd)")
fi
# make does not track these flags: a build made with others starts afresh.
port_build="$mpy_dir/ports/qemu/$build_name"
if [ "$(cat "$port_build/jiro-flags" 2>/dev/null)" != "${port_flags[*]}" ]; then
  rm -rf "$port_build"
fi
build make -C "$mpy_dir/ports/qemu" "${port_flags[@]}" -j"$(nproc)"
echo "${port_flags[*]}" >"$port_build/jiro-flags"
echo "$port_build/firmware.elf"
