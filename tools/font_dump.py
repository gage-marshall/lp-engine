"""Python side of the Rust NFTR loader differential test (engine/src/bin/font_dump.rs).  usage: font_dump.py FONT.NFTR"""
import hashlib, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nftr
f = nftr.Font(open(sys.argv[1], 'rb').read())
print('F cell=%dx%d bpp=%d rot=%d height=%d linefeed=%d alter=%d glyphs=%d chars=%d' % (f.cell_w, f.cell_h, f.bpp, f.rot, f.height, f.linefeed, f.alter, f.nglyphs, len(f.cmap)))
for i in range(f.nglyphs):
    w, h, rows = f.glyph_bitmap(i); l, gw, cw = f.metrics(i)
    print('G %d %d,%d,%d %dx%d %s' % (i, l, gw, cw, w, h, hashlib.sha1(bytes(v for r in rows for v in r)).hexdigest()[:10]))
for c in sorted(f.cmap)[::97]: print('C %d %d' % (c, f.cmap[c]))
