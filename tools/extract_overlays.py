#!/usr/bin/env python3
"""Extract (and BLZ-decompress) overlays from an NDS ROM. Usage: extract_overlays.py rom.nds outdir [arm9|arm7]"""
import struct, sys, os, csv
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from blz_decompress import blz_decode

rom = open(sys.argv[1], 'rb').read(); out = sys.argv[2]; which = sys.argv[3] if len(sys.argv) > 3 else 'arm9'
os.makedirs(out, exist_ok=True)
fat_off, fat_sz = struct.unpack_from('<II', rom, 0x48)
tab_off, tab_sz = struct.unpack_from('<II', rom, 0x50 if which == 'arm9' else 0x58)
rows = []
for i in range(tab_sz // 32):
    oid, ram, size, bss, sinit0, sinit1, fid, comp = struct.unpack_from('<8I', rom, tab_off + 32 * i)
    fo, fe = struct.unpack_from('<II', rom, fat_off + 8 * fid)
    raw = rom[fo:fe]
    csize, flag = comp & 0xFFFFFF, comp >> 24
    data = raw
    if flag & 1:                                  # compressed
        data = blz_decode(raw, csize if csize and csize <= len(raw) else len(raw))
    name = f'{which}_ov{oid:04d}.bin'
    open(os.path.join(out, name), 'wb').write(data)
    rows.append(dict(id=oid, file=name, ram=f'{ram:#010x}', size=size, bss=bss, sinit=f'{sinit0:#x}-{sinit1:#x}',
                     fileid=fid, flag=flag, rawlen=len(raw), declen=len(data), end=f'{ram + len(data):#010x}'))
with open(os.path.join(out, f'{which}_overlays.csv'), 'w', newline='') as f:
    w = csv.DictWriter(f, fieldnames=list(rows[0])); w.writeheader(); w.writerows(rows)
for r in rows: print(r['id'], r['ram'], f"size={r['size']:#x} bss={r['bss']:#x} raw={r['rawlen']:#x} dec={r['declen']:#x} flag={r['flag']} file={r['fileid']}")
print(len(rows), 'overlays, total decompressed bytes', sum(r['declen'] for r in rows))
