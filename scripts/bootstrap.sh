#!/usr/bin/env bash
# Installs the pinned tools that build and check the wedgie app: Rust (rust-toolchain.toml), the
# Arm GNU toolchain and qemu-system-arm (apt), MicroPython v1.29.0 in $JIRO_WEDGIE_HOME (default
# ~/.cache/jiro-wedgie), and pyelftools 0.32 in a venv there, for MicroPython's mpy_ld. Idempotent:
# when everything is in place, the last line printed is "nothing to install". jiro-ikusu's
# scripts/bootstrap.sh, its wedgie tier.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly MICROPYTHON_TAG=v1.29.0
readonly MICROPYTHON_COMMIT=0fd6c573ea815774668bbb16b8e197c8822368b2
readonly PYELFTOOLS_VERSION=0.32
readonly WEDGIE_HOME="${JIRO_WEDGIE_HOME:-$HOME/.cache/jiro-wedgie}"

# Checks must never install a toolchain implicitly: only ensure_rust installs.
export RUSTUP_AUTO_INSTALL=0
export RUST_BACKTRACE=0

installed_something=false

fail() {
  echo "bootstrap: $*" >&2
  exit 1
}

installing() {
  installed_something=true
  echo "bootstrap: installing $*"
}

run_logged() {
  local log
  log="$(mktemp)"
  if ! "$@" >"$log" 2>&1; then
    tail -n 30 "$log" >&2
    rm -f "$log"
    fail "command failed: $*"
  fi
  rm -f "$log"
}

# Usage: ensure <description> <check function> <install function> [arguments for both]
ensure() {
  local description="$1" check="$2" install="$3"
  shift 3
  "$check" "$@" && return
  installing "$description"
  "$install" "$@"
  "$check" "$@" || fail "$description was installed, but the check '$check${*:+ $*}' still fails (is it on PATH?)"
}

toolchain_field() {
  python3 -c 'import sys, tomllib
value = tomllib.load(open(sys.argv[1], "rb"))["toolchain"][sys.argv[2]]
print(" ".join(value) if isinstance(value, list) else value)' "$REPO_ROOT/rust-toolchain.toml" "$1"
}

rust_complete() {
  local toolchain="$1" targets="$2" components="$3" target component
  rustup toolchain list | grep "^$toolchain-" >/dev/null || return 1
  for target in $targets; do
    rustup +"$toolchain" target list --installed | grep -x "$target" >/dev/null || return 1
  done
  for component in $components; do
    rustup +"$toolchain" component list --installed |
      grep -xE "$component(-x86_64-unknown-linux-gnu)?" >/dev/null || return 1
  done
}

install_rust() {
  local toolchain="$1" targets="$2" components="$3"
  run_logged rustup toolchain install "$toolchain" --profile minimal \
    --component "${components// /,}" --target "${targets// /,}" --no-self-update
}

packages_installed() {
  local package
  for package in "$@"; do
    dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -x 'install ok installed' >/dev/null || return 1
  done
}

install_packages() {
  local sudo=()
  [ "$(id -u)" = 0 ] || sudo=(sudo)
  run_logged "${sudo[@]}" apt-get update -qq
  run_logged "${sudo[@]}" apt-get install -y -qq "$@"
}

micropython_cloned() {
  [ "$(git -C "$WEDGIE_HOME/micropython" rev-parse HEAD 2>/dev/null)" = "$MICROPYTHON_COMMIT" ]
}

clone_micropython() {
  rm -rf "$WEDGIE_HOME/micropython"
  mkdir -p "$WEDGIE_HOME"
  run_logged git -c advice.detachedHead=false clone --depth 1 --branch "$MICROPYTHON_TAG" \
    https://github.com/micropython/micropython.git "$WEDGIE_HOME/micropython"
}

# mpy_ld needs pyelftools; a venv, because the runners' system Python refuses global installs.
pyelftools_installed() {
  "$WEDGIE_HOME/venv/bin/python3" -c 'import sys, elftools; sys.exit(elftools.__version__ != sys.argv[1])' \
    "$PYELFTOOLS_VERSION" 2>/dev/null
}

install_pyelftools() {
  run_logged python3 -m venv "$WEDGIE_HOME/venv"
  run_logged "$WEDGIE_HOME/venv/bin/pip" install --quiet "pyelftools==$PYELFTOOLS_VERSION"
}

command -v rustup >/dev/null || fail "rustup is missing"
toolchain="$(toolchain_field channel)"
ensure "Rust $toolchain with its targets and components" rust_complete install_rust \
  "$toolchain" "$(toolchain_field targets)" "$(toolchain_field components)"
ensure "apt packages gcc-arm-none-eabi and qemu-system-arm" packages_installed install_packages \
  gcc-arm-none-eabi qemu-system-arm
ensure "MicroPython $MICROPYTHON_TAG in $WEDGIE_HOME/micropython" micropython_cloned clone_micropython
ensure "pyelftools $PYELFTOOLS_VERSION in $WEDGIE_HOME/venv" pyelftools_installed install_pyelftools

if [ "$installed_something" = true ]; then
  echo "bootstrap: ready"
else
  echo "nothing to install"
fi
