"""Reference note list of a sequence for the first N ticks (the Python interpreter tools/sseq.py); compared with the Rust driver's note_dump.
usage: note_dump.py SOUND_DATA.sdat SEQ_ID TICKS"""
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sdat, sseq

d = open(sys.argv[1], 'rb').read(); sd = sdat.SDAT(d); sid = int(sys.argv[2]); ticks = int(sys.argv[3])
f = sd.file_bytes(sd.info_recs['seq'][sid] and struct.unpack_from('<I', d, sd.info_recs['seq'][sid])[0] & 0xffffff)
res = sseq.run(sseq.parse(f), loops=3, max_seconds=3600.0)
ticks = min(ticks, res.end_tick)
print('#ticks', ticks)
notes = sorted((n['tick'], n['track'], n['key'] - 0, n['vel'], n['prg']) for n in res.notes if n['tick'] < ticks and not False)
for t in notes: print(*t)
