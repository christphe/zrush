#!/usr/bin/env bash
#
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Christophe Opoix
#
# Build zrush from this checkout, then install what came out. Everything
# past the build is install.sh's job, so a checkout and a release archive
# install the same way.
#
#   ./install-dev.sh            ask which editor, or keep the configured one
#   ./install-dev.sh zed        no questions
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

command -v cargo >/dev/null 2>&1 || {
  echo "cargo not found. Install Rust: https://rustup.rs" >&2
  exit 1
}

echo "building (release)…"
cargo build --release --locked --manifest-path "$here/Cargo.toml" -p zrush

exec "$here/install.sh" --bin "$here/target/release/zrush" "$@"
