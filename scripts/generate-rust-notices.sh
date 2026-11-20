#!/bin/sh
# Regenerate THIRD_PARTY_RUST_LICENSES.md, or verify it with --check.
# Requires: cargo install cargo-about --locked --version 0.9.3 --features cli
set -eu

project_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_root"
output=THIRD_PARTY_RUST_LICENSES.md
generated=$(mktemp)
trap 'rm -f "$generated"' EXIT
# Skip DornGuard's own workspace crates so version bumps do not change the file.
cargo about generate --locked about.hbs | grep -v '^- dornguard-' > "$generated"

if [ "${1:-}" = "--check" ]; then
    if ! diff -u "$output" "$generated"; then
        echo "$output is out of date; run scripts/generate-rust-notices.sh" >&2
        exit 1
    fi
else
    cp "$generated" "$output"
fi
