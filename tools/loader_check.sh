#!/bin/sh
# Diff the Rust loaders against the Python exporters (layer 1 vs layer 2).  usage: tools/loader_check.sh [CONTENT] [EXPORT]
set -e
C=${1:-content}; X=${2:-export}; T=$(mktemp -d)
cargo build --release --manifest-path engine/Cargo.toml -q
engine/target/release/text_dump "$C/assets/text" "$T/text"
python3 tools/tree_diff.py "$X/text" "$T/text"
engine/target/release/nns2d_dump "$C/assets" "$T/nns2d"
python3 tools/tree_diff.py "$X/graphics/nns" "$T/nns2d"
rm -rf "$T"
