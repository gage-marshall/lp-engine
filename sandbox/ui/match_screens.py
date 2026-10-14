"""Score every NSCR/NCGR of an archive against a captured BG layer: map entries equal under one tile-index and palette
offset; tiles byte-equal at an offset in the char block.
    .venv/bin/python sandbox/ui/match_screens.py CAPTURE_DIR BG ARCHIVE_DIR"""
import collections, glob, json, os, struct, sys
sys.path.insert(0, 'tools')
import nns2d  # noqa: E402
d, n, arch = sys.argv[1], int(sys.argv[2]), sys.argv[3]
r = json.load(open(os.path.join(d, 'regs.json')))
vram = open(os.path.join(d, 'vram_bg_main.bin'), 'rb').read()
cnt = (r['main']['008' if n < 2 else '00c'] >> (16 * (n & 1))) & 0xffff
char, scr = (cnt >> 2 & 15) * 0x4000, (cnt >> 8 & 31) * 0x800
size = [0x800, 0x1000, 0x1000, 0x2000][cnt >> 14]
vm = struct.unpack('<%dH' % (size // 2), vram[scr:scr + size])
res = []
for f in sorted(glob.glob(os.path.join(arch, '*.nscr'))):
    sc = nns2d.parse_nscr(open(f, 'rb').read())
    fm = struct.unpack('<%dH' % (len(sc['raw']) // 2), sc['raw'])
    if len(fm) != len(vm):
        continue
    deltas = collections.Counter(((v & 0x3ff) - (w & 0x3ff), (v >> 12) - (w >> 12)) for v, w in zip(vm, fm) if v & 0xc00 == w & 0xc00)
    (dt, dp), c = deltas.most_common(1)[0]
    res.append((c / len(vm), f, sc['w'], sc['h'], dt, dp))
for x in sorted(res, reverse=True)[:6]:
    print('NSCR %.3f %s %dx%d tile+%d pal+%d' % x)
for f in sorted(glob.glob(os.path.join(arch, '*.ncgr'))):
    ch = nns2d.parse_ncgr(open(f, 'rb').read())
    raw = ch['raw']
    if ch['bpp'] != 4:
        continue
    nt = len(raw) // 32
    # where in the char block does this file's tile 0..nt sit?
    best = None
    for start in range(0, 0x4000 * 4 - 32, 32):
        k = min(nt, 64)
        if vram[char + start:char + start + 32 * k] == raw[:32 * k]:
            best = start // 32; break
    if best is not None:
        print('NCGR %s %d tiles at char tile %d' % (f, nt, best))
