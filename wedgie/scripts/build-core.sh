#!/usr/bin/env bash
# Builds jiro-natmod (wedgie/natmod/rust) for thumbv6m-none-eabi, then partially links its
# staticlib into one relocatable object that keeps only what the exported entry points reach
# (mpy_ld has no section garbage collection), without debug, unwind-index and bitcode sections.
# Usage: build-core.sh <out.o>
set -euo pipefail

[ "$#" -eq 1 ] || { echo "usage: build-core.sh <out.o>" >&2; exit 2; }
out="$1"
rust_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../natmod/rust" && pwd)"
readonly ENTRY_POINTS="jiro_rules_version jiro_replay jiro_recorder_size jiro_recorder_align
  jiro_recorder_start jiro_recorder_step jiro_recorder_view jiro_status_len jiro_status jiro_score
  jiro_frame_bytes jiro_shadow_size jiro_paint jiro_restore"

# Cargo reads .cargo/config.toml, and rustup rust-toolchain.toml, from the working directory.
(cd "$rust_dir" && cargo build --release --locked)
mkdir -p "$(dirname "$out")"
undefined=()
for symbol in $ENTRY_POINTS; do
  undefined+=(-u "$symbol")
done
arm-none-eabi-ld -r --gc-sections "${undefined[@]}" -o "$out.full" \
  "$rust_dir/../../../target/thumbv6m-none-eabi/release/libjiro_natmod.a"
arm-none-eabi-objcopy --strip-debug --remove-section='.ARM.exidx*' --remove-section=.llvmbc \
  --remove-section=.llvmcmd --remove-section=.comment "$out.full" "$out"
rm "$out.full"
arm-none-eabi-size "$out"
