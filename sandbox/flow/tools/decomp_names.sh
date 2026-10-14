#!/bin/bash
# Decompile static functions by label: decomp_names.sh OUTNAME Name [Name ...] -> fid/decomp/OUTNAME.c
cd "$(dirname "$0")/../../.."
out=$1; shift
ranges=""
for n in "$@"; do
  a=$(grep -m1 "^static,[0-9a-f]*,$n," fid/labels_all.csv | cut -d, -f2)
  if [ -z "$a" ]; then a=$(printf "%08x" $((16#${n#0x}))) ; fi
  ranges="$ranges $(printf '%x-%x' $((16#$a)) $((16#$a+1)))"
done
bash tools/decomp_ranges.sh "$out" $ranges 2>&1 | tail -1
