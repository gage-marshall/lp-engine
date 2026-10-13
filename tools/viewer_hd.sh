#!/bin/bash
# Enhanced-mode render experiments: authentic vs enhanced looks at 1440p / 4K.  usage: tools/viewer_hd.sh [CONTENT] [OUTDIR]
C=${1:-content}; O=${2:-export/viewer/hd}; V=engine/target/release/viewer; mkdir -p "$O"
for h in m r n; do
  for look in authentic clean-s soft-s; do
    $V "$C" --heroine $h --outfit 8 --motion 20 --frame 6 --mood 3 --width 2560 --height 1440 --ss 2 --zoom 2.4 --focus-y 105 --look $look --png "$O/${h}_${look}_1440p.png" >/dev/null
  done
  for look in clean-s soft-s; do
    $V "$C" --heroine $h --outfit 8 --motion 20 --frame 6 --mood 3 --width 3840 --height 2160 --ss 2 --zoom 2.4 --focus-y 105 --look $look --png "$O/${h}_${look}_4k.png" >/dev/null
  done
done
echo done
