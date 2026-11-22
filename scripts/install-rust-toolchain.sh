#!/bin/sh
# Install the Rust toolchain named in .github/rust-version and make it the default.
set -eu

project_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
channel=$(grep -v '^#' "$project_root/.github/rust-version" | head -n 1 | tr -d '[:space:]')
if [ -z "$channel" ]; then
    echo "No Rust version in .github/rust-version" >&2
    exit 1
fi
"$project_root/scripts/retry.sh" rustup toolchain install "$channel" \
    --profile minimal --component clippy --component rustfmt
rustup default "$channel"
rustc --version
cargo clippy --version
