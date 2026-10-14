"""Which asset files make a captured BG layer: the NSCR whose map best equals the layer's map (same size or the layer's
top-left part), the NCGR whose tiles sit in the char block, the NCLR rows in the palette.
    .venv/bin/python sandbox/ui/match_layer.py CAPTURE_DIR ENGINE BG ARCHIVE_DIR..."""
import glob, os, struct, sys
sys.path.insert(0, 'tools'); sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nns2d, layers  # noqa: E402
d, eng, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
r, pal, vram, _, _ = layers.load(d, eng)
cnt = (r['main']['008' if n < 2 else '00c'] >> (16 * (n & 1))) & 0xffff
dc = r['main']['000']
char = (cnt >> 2 & 15) * 0x4000 + ((dc >> 24 & 7) * 0x10000 if eng == 'main' else 0)
scr = (cnt >> 8 & 31) * 0x800 + ((dc >> 27 & 7) * 0x10000 if eng == 'main' else 0)
w, h = [(256, 256), (512, 256), (256, 512), (512, 512)][cnt >> 14]
# the layer map in row-major 2D (handles the 2x1 / 1x2 block order)
def entry(x, y):
    blk = (x // 32) + (y // 32) * (w // 256)
    return struct.unpack_from('<H', vram, scr + blk * 0x800 + ((y % 32) * 32 + x % 32) * 2)[0]
L = [[entry(x, y) for x in range(w // 8)] for y in range(h // 8)]
pals = sorted({e >> 12 for row in L for e in row if e & 0x3ff})
print('BG%d %s: %dx%d char %#x screen %#x palettes %s' % (n, eng, w, h, char, scr, pals))
best = []
for arch in sys.argv[4:]:
    for f in sorted(glob.glob(os.path.join(arch, '*.nscr'))):
        sc = nns2d.parse_nscr(open(f, 'rb').read())
        fw, fh = sc['w'] // 8, sc['h'] // 8
        m = struct.unpack('<%dH' % (len(sc['raw']) // 2), sc['raw'])
        if fw > w // 8 or fh > h // 8 or fw * fh == 0:
            continue
        import collections
        dl = collections.Counter(((L[y][x] & 0x3ff) - (m[y * fw + x] & 0x3ff)) for y in range(fh) for x in range(fw) if m[y * fw + x] & 0x3ff and L[y][x] & 0xc00 == m[y * fw + x] & 0xc00)
        off = dl.most_common(1)[0][0] if dl else 0
        same = sum(((L[y][x] & 0x3ff) == ((m[y * fw + x] & 0x3ff) + off if m[y * fw + x] & 0x3ff else 0)) for y in range(fh) for x in range(fw))
        f = '%s (tile +%d)' % (f, off)
        tot = fw * fh
        nz = sum(1 for y in range(fh) for x in range(fw) if m[y * fw + x] & 0x3ff)
        best.append((same / tot, nz, f, fw * 8, fh * 8))
for b in sorted(best, reverse=True)[:4]:
    print('  NSCR %.3f (nonzero %d) %s %dx%d' % b)
for arch in sys.argv[4:]:
    for f in sorted(glob.glob(os.path.join(arch, '*.ncgr'))):
        ch = nns2d.parse_ncgr(open(f, 'rb').read()); raw = ch['raw']
        if ch['bpp'] != 4 or len(raw) < 64: continue
        k = min(len(raw), 32 * 32)
        i = vram.find(raw[32:k], char, char + 0x10000)
        if i >= 0:
            print('  NCGR %s %d tiles at char tile %d' % (f, len(raw) // 32, (i - 32 - char) // 32))
    for f in sorted(glob.glob(os.path.join(arch, '*.nclr'))):
        b = open(f, 'rb').read()[40:]
        for p in pals:
            row = pal[p * 32 + 2:p * 32 + 32]
            for k in range(0, len(b) - 30, 32):
                if b[k + 2:k + 32] == row:
                    print('  NCLR %s row %d -> palette %d' % (f, k // 32, p)); break
