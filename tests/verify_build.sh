#!/usr/bin/env bash
set -euo pipefail
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
qa=$(mktemp -d "${TMPDIR:?TMPDIR must point to a private scratch directory}/glacier-build-qa.XXXXXXXX")
trap 'rm -rf -- "$qa"' EXIT
export XDG_DATA_HOME="$qa/data"
mkdir -p "$qa/bin"
printf '#!/bin/sh\nexit 0\n' > "$qa/bin/notify-send"
chmod 700 "$qa/bin/notify-send"
export PATH="$qa/bin:$PATH"
cargo fmt --check
cargo fmt --manifest-path tests/clip_probe/Cargo.toml --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
CARGO_TARGET_DIR="$PWD/target/probe" cargo clippy --manifest-path tests/clip_probe/Cargo.toml --all-targets -- -D warnings
CARGO_TARGET_DIR="$PWD/target/probe" cargo build --release --manifest-path tests/clip_probe/Cargo.toml
printf 'G3_BUILD_OK\n'
