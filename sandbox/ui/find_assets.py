"""Find the asset files whose data is in a captured BG layer: NSCR map runs and NCGR tiles (raw byte search).
    .venv/bin/python sandbox/ui/find_assets.py CAPTURE_DIR BG [search_root]"""
import glob, json, os, struct, sys
d, n = sys.argv[1], int(sys.argv[2])
root = sys.argv[3] if len(sys.argv) > 3 else 'content/assets'
r = json.load(open(os.path.join(d, 'regs.json')))
vram = open(os.path.join(d, 'vram_bg_main.bin'), 'rb').read()
cnt = (r['main']['008' if n < 2 else '00c'] >> (16 * (n & 1))) & 0xffff
char, scr = (cnt >> 2 & 15) * 0x4000, (cnt >> 8 & 31) * 0x800
size = [0x800, 0x1000, 0x1000, 0x2000][cnt >> 14]
smap = vram[scr:scr + size]
words = struct.unpack('<%dH' % (size // 2), smap)
used = sorted({w & 0x3ff for w in words})
print('BG%d: %d map entries, %d distinct tiles, tile range %d..%d, palettes %s' % (n, len(words), len(used), used[0], used[-1], sorted({w >> 12 for w in words})))
# map: look for a run of 32 entries (one row) with the tile index offset removed
rows = []
for y in range(size // 64):
    row = words[y * 32:(y + 1) * 32]
    if len(set(row)) > 4:
        rows.append((y, row))
files = [f for f in glob.glob(os.path.join(root, '**/*'), recursive=True) if os.path.isfile(f) and f.endswith(('.nscr', '.ncgr', '.bin'))]
print(len(files), 'files to search')
for f in files:
    b = open(f, 'rb').read()
    if f.endswith('.nscr') or f.endswith('.bin'):
        for y, row in rows[:6]:
            for off in (0,):
                pat = struct.pack('<32H', *[w & 0xf3ff for w in row])
                pat2 = struct.pack('<16H', *[w & 0x3ff for w in row[:16]])
                if pat in b or pat2 in b:
                    print('MAP row %d in %s' % (y, f)); break
    if f.endswith('.ncgr') or f.endswith('.bin'):
        hits = 0
        for t in used[1:40]:
            tile = vram[char + t * 32:char + t * 32 + 32]
            if len(set(tile)) > 3 and tile in b:
                hits += 1
        if hits >= 5:
            print('TILES %d/39 in %s' % (hits, f))
