"""The scene dialogue window rebuilt from the game's files (menu/mount_menu.bin), checked against a capture.

Frame: BG3 = 0079.nscr (256x192 map; variants 0080-0101/0113 = button and name-tab states) over 0072.ncgr tiles (char
block 3, tile 0) with 0077.nclr row 0 in BG palette 13. Text: BG2 (char block 3, palette 15 = white ink), 3 lines at a 16 px
pitch from x 8, name in the tab at x 57 (sideways DS coordinates). Blend (BLDCNT 0x1348, BLDALPHA 0x080a): BG3 is the
first target at 10/16 over BG0 (3D) / BG1 (backdrop) / OBJ at 8/16, saturating at 31.
    .venv/bin/python sandbox/ui/window.py CAPTURE_DIR"""
import os, struct, sys
from PIL import Image
sys.path.insert(0, 'tools')
import nns2d  # noqa: E402
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import layers  # noqa: E402

MM = 'content/assets/menu/mount_menu.bin'


def frame_layer(variant=79):
    """BG3 from the files: 256x192 RGBA (transparent where colour 0)"""
    sc = nns2d.parse_nscr(open('%s/%04d.nscr' % (MM, variant), 'rb').read())
    ch = nns2d.parse_ncgr(open('%s/0072.ncgr' % MM, 'rb').read())
    pl = open('%s/0077.nclr' % MM, 'rb').read()[40:40 + 32]
    cols = [layers.rgb555(struct.unpack_from('<H', pl, 2 * i)[0]) for i in range(16)]
    raw = ch['raw']
    im = Image.new('RGBA', (256, 192), (0, 0, 0, 0)); px = im.load()
    m = struct.unpack('<%dH' % (len(sc['raw']) // 2), sc['raw'])
    for ty in range(24):
        for tx in range(32):
            e = m[ty * 32 + tx]; t, hf, vf = e & 0x3ff, e >> 10 & 1, e >> 11 & 1
            for y in range(8):
                for x in range(8):
                    sx, sy = (7 - x if hf else x), (7 - y if vf else y)
                    b = raw[t * 32 + sy * 4 + sx // 2]; c = b >> 4 if sx & 1 else b & 15
                    if c: px[tx * 8 + x, ty * 8 + y] = cols[c]
    return im


def blend(under, win, eva=10, evb=8):
    """DS alpha blend of 5-bit channels: min(31, (a*eva + b*evb) / 16)"""
    u, w = under.load(), win.load()
    out = under.copy(); o = out.load()
    for y in range(under.height):
        for x in range(under.width):
            if w[x, y][3]:
                a = [c * 31 // 255 for c in w[x, y][:3]]; b = [c * 31 // 255 for c in u[x, y][:3]]
                o[x, y] = tuple(min(31, (p * eva + q * evb) // 16) * 255 // 31 for p, q in zip(a, b)) + (255,)
    return out


if __name__ == '__main__':
    d = sys.argv[1]
    r, pal, bg, obj, oam = layers.load(d)
    vram3, _ = layers.bg_layer(r, pal, bg, 3)
    mine = frame_layer()
    a, b = vram3.crop((0, 0, 256, 192)).load(), mine.load()
    diff = sum(a[x, y] != b[x, y] for x in range(256) for y in range(192))
    print('frame from files vs captured BG3: %d / %d pixels differ' % (diff, 256 * 192))
    # the composed picture at native resolution: backdrop (BG1) under the blended window, text (BG2) on top
    bg1, _ = layers.bg_layer(r, pal, bg, 1)
    base = Image.new('RGBA', (256, 192), (0, 0, 0, 255)); base.alpha_composite(bg1.crop((0, 0, 256, 192)))
    out = blend(base, mine)
    bg2, _ = layers.bg_layer(r, pal, bg, 2)
    out.alpha_composite(bg2.crop((0, 0, 256, 192)))
    out.rotate(90, expand=True).save(os.path.join(d, 'window_composed.png'))
    print('->', os.path.join(d, 'window_composed.png'))
