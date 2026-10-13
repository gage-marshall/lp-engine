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
echo "== cross-script machine (every directory event)"
$B/machine_dump "$C" > "$T/rm.txt"; python3 tools/machine_dump.py "$C" > "$T/pm.txt"; cmp "$T/rm.txt" "$T/pm.txt" && echo "identical ($(wc -l < "$T/rm.txt") lines)"
echo "== SDAT parser, voice decoder, sequencer"
$B/sdat_dump "$C/assets/sys/sound_data.sdat" > "$T/rs.txt"; python3 tools/sdat_dump.py "$C/assets/sys/sound_data.sdat" > "$T/ps.txt"; cmp "$T/rs.txt" "$T/ps.txt" && echo "sdat identical ($(wc -l < "$T/rs.txt") lines)"
clang -O2 -o "$T/siren14dec" tools/siren14dec.c 2>/dev/null
for clip in 0 1 3206 5736 19202 33103; do
  $B/siren_dump "$C" $clip > "$T/sr.txt"; "$T/siren14dec" "$C/engine/g7221.pack" "$C/assets/sys/Siren14Voice.vtb" "$C/assets/sys/Siren14Voice.spp" $clip "$T/c.wav" 2>/dev/null
  python3 - "$T/sr.txt" "$T/c.wav" <<'PY'
import hashlib, sys
r = open(sys.argv[1]).read().split(); d = open(sys.argv[2], 'rb').read()[44:]
assert r[3] == hashlib.sha1(d).hexdigest()[:16] and int(r[1]) == len(d) // 2, ('voice mismatch', r)
PY
done; echo "voice clips identical"
for id in 1 2 5 17 51 80 132; do
  python3 tools/note_dump.py "$C/assets/sys/sound_data.sdat" $id 3000 > "$T/np.txt"; n=$(head -1 "$T/np.txt" | cut -d' ' -f2); tail -n +2 "$T/np.txt" > "$T/np2.txt"
  $B/note_dump "$C/assets/sys/sound_data.sdat" $id $n > "$T/nr.txt"; cmp "$T/nr.txt" "$T/np2.txt" || { echo "sequence $id differs"; exit 1; }
done; echo "sequencer note streams identical"
echo "== viewer vs reference rasteriser (needs $X/camera/presets.json for the light angles)"
for h in m r n; do python3 tools/viewer_parity.py "$C" $B/viewer --heroine $h --outfit 3; done
echo "== cargo test"; cargo test --release --manifest-path engine/Cargo.toml -q 2>&1 | grep "test result"
rm -rf "$T"
