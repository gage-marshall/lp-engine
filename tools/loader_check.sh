#!/bin/sh
# Diff the Rust loaders against the Python reference code (layer 1 vs layer 2) and run the Rust tests.
# usage: tools/loader_check.sh [CONTENT] [EXPORT]      (the full G3D diff takes a few minutes: Python is the slow side)
set -e
C=${1:-content}; X=${2:-export}; T=$(mktemp -d)
B=engine/target/release
cargo build --release --manifest-path engine/Cargo.toml -q
echo "== TXRC text";    $B/text_dump "$C/assets/text" "$T/text";           python3 tools/tree_diff.py "$X/text" "$T/text"
echo "== NNS 2D";       $B/nns2d_dump "$C/assets" "$T/nns2d";              python3 tools/tree_diff.py "$X/graphics/nns" "$T/nns2d"
echo "== G3D models, textures, animations"
$B/nns3d_dump "$C/assets" > "$T/r3d.txt"; python3 tools/nns3d_dump.py "$C/assets" > "$T/p3d.txt"; cmp "$T/r3d.txt" "$T/p3d.txt" && echo "identical ($(wc -l < "$T/r3d.txt") lines)"
echo "== heroine assembly"
$B/heroine_dump "$C/assets/arc" "$C/engine/face_tables.json" > "$T/rh.txt"; python3 tools/heroine_dump.py "$C/assets/arc" "$C/engine/face_tables.json" > "$T/ph.txt"; cmp "$T/rh.txt" "$T/ph.txt" && echo identical
echo "== viewer vs reference rasteriser (needs $X/camera/presets.json for the light angles)"
for h in m r n; do python3 tools/viewer_parity.py "$C" $B/viewer --heroine $h --outfit 3; done
echo "== cargo test"; cargo test --release --manifest-path engine/Cargo.toml -q 2>&1 | grep "test result"
rm -rf "$T"
