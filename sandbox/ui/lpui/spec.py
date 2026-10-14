"""Screen specs: everything a screen shows, as data, rendered without RAM or captured VRAM.

  loads     file loads still in effect (lpui.screens recipe ops: memory, offset, size, archive / entry / offset, zero fills)
  canvases  text canvases on show (lpui.canvases: geometry, Text_DrawString draw list, the box's own string state)
  sprites   sprite objects in submission order (raw 0x38-byte object + the cell / animation files their handles name);
            OAM is built from the files (cell = NANR(sequence, time)) with lpui.sprites' rules
  regs      display registers, VRAM bank mapping, scroll, blending, brightness (from the capture: per-screen constants
            or the state of a screen-logic module)
  affine    OAM affine parameters (rotation / scale of affine sprites)

extract(trace) yields one spec per capture; render(spec) composes both screens; the replay check compares that render with
the capture's own composite (ds2d of its real VRAM), which is exact for 149 / 208 captures by itself (emulator-free)."""
import base64, json, os, struct, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
import ds2d  # noqa: E402
from lpui import canvases, cells, hudprobe, screens, sprites, textbox  # noqa: E402
from lpui.vram import Vram, file_bytes  # noqa: E402

B = 0x2000000


def _sprite_specs(ram, bufs, frame):
    out = []
    for li in range(0x3f, -1, -1):
        p = struct.unpack_from('<I', ram, 0x213f6a0 - B + 4 * li)[0]
        guard = 0
        while B <= p < B + 0x400000 and guard < 256:
            guard += 1
            o = sprites.SprObj(ram, p)
            if o.kind in (2, 3) and o.cell:
                carc, cent, _ = hudprobe.resource_file(ram, o.cell_bank, bufs, frame)
                aarc, aent, _ = hudprobe.resource_file(ram, o.anim_bank, bufs, frame)
                ok = bool(carc and aarc and file_bytes(carc, cent) and file_bytes(aarc, aent))
                d = dict(obj=ram[p - B:p - B + 0x38].hex(), cells=[carc, cent] if ok else None, anim=[aarc, aent] if ok else None)
                if not ok:              # file not resolved: keep the cell's OAMs as data
                    d['oams'] = sprites.cell_oams(ram, o.cell)
                out.append(d)
            p = o.next
    return out


def _canvas_spec(sp):
    bx = sp.box
    return dict(addr=bx.addr, w=bx.w, h=bx.h, char_ofs=bx.char_ofs, char_ptr=bx.char_ptr, scr_ptr=bx.scr_ptr,
                string=bx.string, drawn=bx.drawn, done=bx.done, start=list(bx.start), left=bx.left, top=bx.top,
                draws=[list(d) for d in sp.draws], colour=1 if bx.addr in DIALOGUE_LINES else 2)


DIALOGUE_LINES = (0x2365880, 0x2365b28, 0x2365dd0)     # the scene dialogue's three line boxes draw in colour 1


def extract(trace_dir):
    """yield (frame, capture path, spec dict)"""
    bufs = hudprobe.loaded_buffers(trace_dir)
    first = {}
    for fr, ptr, arc, entry in bufs:
        first.setdefault((arc, entry), fr)
    loaded = sorted(((a, e, fr) for (a, e), fr in first.items()), key=lambda x: -x[2])   # most recent first
    cw = canvases.walk(trace_dir)
    for (f, path, ops, active), (f2, path2, draws) in zip(screens.walk(trace_dir), cw):
        assert f == f2
        cap = ds2d.Capture(path)
        ram = open(os.path.join(path, 'ram.bin'), 'rb').read()
        vc = bytes(cap.regs['vramcnt']).hex()
        loads = [dict(mem=op['mem'], at=op['at'], size=op['size'], origin=op.get('origin'), zero=op.get('zero', False))
                 for op in ops if op.get('vc') in (None, vc) and (op.get('origin') or op.get('zero'))]
        cvs = []
        for sp in canvases.specs(ram, draws):
            if sp.on_screen(np.frombuffer(cap.vram[sp.box.mem], np.uint8)):
                cvs.append(_canvas_spec(sp))
        oam = cap.oam
        affine = {eng: [struct.unpack_from('<h', oam, base + 8 * i + 6)[0] for i in range(128)] for eng, base in (('main', 0), ('sub', 0x400))}
        spec = dict(frame=f, loads=loads, canvases=cvs, sprites=_sprite_specs(ram, bufs, f), affine=affine,
                    regs=cap.regs, scroll=getattr(cap, 'scroll_regs', None), dtcm_scroll=_dtcm_scroll(cap), touch=active[0] if active else [])
        import collections
        built, _ = build_oam(spec)
        spec['oam_residual'] = {}
        for eng, base in (('main', 0), ('sub', 0x400)):
            mine = collections.Counter(struct.unpack_from('<3H', built, base + 8 * i) for i in range(128))
            res = []
            for i in range(128):
                e = struct.unpack_from('<3H', cap.oam, base + 8 * i)
                if not visible_entry(e):
                    continue
                if mine[e] > 0:
                    mine[e] -= 1
                else:
                    res.append([i, list(e)])
            spec['oam_residual'][eng] = res
        actual = {k: np.frombuffer(cap.vram[k], np.uint8) for k in ('bg_main', 'bg_sub', 'obj_main', 'obj_sub')}
        actual['pal'] = np.frombuffer(cap.pal.tobytes(), np.uint8)
        tp = np.frombuffer(text_palette(), np.uint8)
        spec['text_palette'] = [eng for eng, o in (('main', 15 * 32), ('sub', 0x400 + 15 * 32)) if (actual['pal'][o:o + 32] == tp).all()]
        spec['residual'] = {}
        _detect_map_edits(spec, actual)
        v, owned = _compose(spec)
        _infer_file_runs(spec, actual, owned, v, [((a, e), file_bytes(a, e)) for a, e, fr in loaded if fr <= f and file_bytes(a, e)])
        v, owned = _compose(spec)
        for k, act in actual.items():
            d = (v.mem[k] != act) & ~owned[k]
            spec['residual'][k] = _runs(d, act)
        _prune(spec)
        yield f, path, spec


def _map_add(mem, at, size, dtile, dpal):
    """TileMap_AddTileIndexOffset / TileMap_AddPaletteOffset on a loaded map: tile index += dtile, palette += dpal"""
    e = mem[at:at + size // 2 * 2].view('<u2')
    t = ((e & 0x3ff) + dtile) & 0x3ff
    pl = ((e >> 12) + dpal) & 15
    mem[at:at + size // 2 * 2] = ((e & 0x0c00) | t | (pl << 12)).astype('<u2').view(np.uint8)


def _detect_map_edits(spec, actual):
    """loads whose bytes on screen are the file's map entries plus one constant tile / palette offset"""
    import collections
    for op in spec['loads']:
        o = op.get('origin')
        if not o or op['mem'] not in ('bg_main', 'bg_sub') or op['size'] < 64 or op['size'] % 2:
            continue
        fb = file_bytes(o['arc'], o.get('entry', -1))
        if fb is None or o['off'] + op['size'] > len(fb):
            continue
        a = actual[op['mem']][op['at']:op['at'] + op['size']].view('<u2')
        f = np.frombuffer(fb[o['off']:o['off'] + op['size']], '<u2')
        if (a == f).all() or not ((a & 0xc00) == (f & 0xc00)).all():
            continue
        dt = collections.Counter(zip((((a & 0x3ff).astype(int) - (f & 0x3ff)) & 0x3ff).tolist(), (((a >> 12).astype(int) - (f >> 12)) & 15).tolist()))
        (k, n), = dt.most_common(1)
        if n == len(a) and k != (0, 0):
            op['map_add'] = list(k)


def _infer_file_runs(spec, actual, owned, v, files):
    """residual bytes in tile memories that are a file's bytes at a fixed displacement (copies the trace did not hook,
    e.g. VBlank transfers): become load ops marked inferred (provenance by content, tile-aligned 32-byte chunks)"""
    for k in ('obj_sub', 'obj_main', 'bg_sub', 'bg_main'):
        act = actual[k]
        d = (v.mem[k] != act) & ~owned[k]
        tiles = np.nonzero(d.reshape(-1, 32).any(1))[0]
        i = 0
        while i < len(tiles):
            t = int(tiles[i])
            chunk = act[t * 32:t * 32 + 32].tobytes()
            if not any(chunk):
                i += 1; continue
            hit = None
            for key, blob in files:
                j = blob.find(chunk)
                if j >= 0:
                    hit = (key, j); break
            if not hit:
                i += 1; continue
            (arc, entry), j = hit
            blob = file_bytes(arc, entry)
            n = 1                       # extend while the following tiles keep matching the file
            while (t + n) * 32 + 32 <= len(act) and j + n * 32 + 32 <= len(blob) and \
                    act[(t + n) * 32:(t + n) * 32 + 32].tobytes() == blob[j + n * 32:j + n * 32 + 32]:
                n += 1
            op = dict(mem=k, at=t * 32, size=n * 32, origin=dict(arc=arc, entry=entry, off=j), zero=False, inferred=True)
            spec['loads'].append(op)
            v.apply(op)
            while i < len(tiles) and tiles[i] < t + n:
                i += 1


def _prune(spec):
    """drop residual runs the screens do not show (render unchanged without them)"""
    full = [x for x, u in render(spec)]
    for k in list(spec['residual']):
        runs = spec['residual'][k]
        if not runs:
            continue
        spec['residual'][k] = []
        if all((a == b).all() for a, b in zip(full, [x for x, u in render(spec)])):
            continue
        spec['residual'][k] = runs
        big = [r for r in runs if len(r[1]) >= 2048]
        for r in big:
            spec['residual'][k] = [x for x in spec['residual'][k] if x is not r]
            if not all((a == b).all() for a, b in zip(full, [x for x, u in render(spec)])):
                spec['residual'][k].append(r)
        spec['residual'][k].sort()


def visible_entry(e):
    """an OAM entry that can put pixels on screen (not disabled, rectangle (doubled when affine-double) meets 256 x 192)"""
    a0, a1, a2 = e
    if (a0 >> 8) & 3 == 2:
        return False
    w, h = sprites.SIZE.get((a0 >> 14, a1 >> 14), (8, 8))
    if (a0 >> 8) & 3 == 3:
        w, h = 2 * w, 2 * h
    x, y = a1 & 0x1ff, a0 & 0xff
    x = x - 512 if x >= 256 else x
    y = y - 256 if y + h > 256 else y
    return x < 256 and x + w > 0 and y < 192 and y + h > 0


def _runs(mask, data, gap=16):
    """[(start, hex bytes)] covering the True positions (runs merged across small gaps)"""
    idx = np.nonzero(mask)[0]
    out = []
    if len(idx):
        s = p = int(idx[0])
        for x in idx[1:]:
            x = int(x)
            if x > p + gap:
                out.append((s, data[s:p + 1].tobytes().hex())); s = x
            p = x
        out.append((s, data[s:p + 1].tobytes().hex()))
    return out


def residual_bytes(spec):
    r = {k: sum(len(h) // 2 for s, h in v) for k, v in spec['residual'].items()}
    r.update({'oam_' + e: 8 * len(v) for e, v in spec.get('oam_residual', {}).items()})
    return r


def _dtcm_scroll(cap):
    return {'%s%d' % (e, n): list(cap.scroll(e, n)) for e in ('main', 'sub') for n in range(4)}


class _Box:
    """a TextBox from a canvas spec (no RAM)"""
    def __init__(self, c):
        self.addr, self.w, self.h, self.char_ofs = c['addr'], c['w'], c['h'], c['char_ofs']
        self.char_ptr, self.scr_ptr, self.string = c['char_ptr'], c['scr_ptr'], c['string']
        self.drawn, self.done, self.start = c['drawn'], c['done'], tuple(c['start'])
        self.left, self.top = c['left'], c['top']
    mem = textbox.TextBox.mem
    vram_off = staticmethod(textbox.TextBox.vram_off)
    mapped = textbox.TextBox.mapped
    canvas = textbox.TextBox.canvas


def build_oam(spec):
    """both engines' OAM (1 KB each) from the sprite specs, unused entries hidden"""
    def cell_of(o, d):
        if d['cells']:
            return cells.cell_oams(*d['cells'], cells.anim_cell(cells.nanr(*d['anim']), o.seq, o.time))
        return [tuple(x) for x in d['oams']]
    out = {'main': [], 'sub': []}
    aff = {'main': 0, 'sub': 0}
    for d in spec['sprites']:
        raw = bytes.fromhex(d['obj'])
        o = sprites.SprObj(raw, B)
        eng = 'main' if o.engine == 1 else 'sub'
        idx = None
        if o.kind == 2:
            if B <= o.mtx < B + 0x400000:
                idx = aff[eng]; aff[eng] += 1
            out[eng] += sprites.build(o, cell_of(o, d), idx)
        else:
            sx, sy, rot = struct.unpack_from('<hhH', raw, 0x10)
            if rot or (sx, sy) not in ((0x1000, 0x1000), (0, 0)) or raw[0x16]:
                idx = aff[eng]; aff[eng] += 1
            out[eng] += sprites.build_transformed(o, raw, cell_of(o, d), idx)
    oam = bytearray(0x800)
    for eng, base in (('main', 0), ('sub', 0x400)):
        res = {i: tuple(e) for i, e in spec.get('oam_residual', {}).get(eng, [])}     # entries other submitters put at index i
        ours = iter(out[eng])
        ents = []
        for i in range(128):
            if i in res:
                ents.append(res[i])
            else:
                ents.append(next(ours, (0x200, 0, 0)))
        for i in range(128):
            a0, a1, a2 = ents[i] if i < len(ents) else (0x200, 0, 0)
            struct.pack_into('<4H', oam, base + 8 * i, a0, a1, a2, spec['affine'][eng][i] & 0xffff)
    return bytes(oam), {k: len(v) for k, v in out.items()}


class _Scroll:
    def __init__(self, spec):
        self.s = spec['dtcm_scroll']

    def __call__(self, eng, n):
        return tuple(self.s['%s%d' % (eng, n)])


TEXT_PALETTE = 0x2126e58          # static table: the text colours (LoadBGPalette15_BothScreens loads it every frame)


def text_palette():
    d = open(os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(HERE))), 'work/arm9_main.bin'), 'rb').read()
    return d[TEXT_PALETTE - B:TEXT_PALETTE - B + 32]


def _compose(spec, residual=False):
    """(Vram, {memory: mask of the bytes widgets own}): loads, [residual], text palette, canvases"""
    oam, counts = build_oam(spec)
    v = Vram(regs=spec['regs'], oam=oam, scroll_regs=spec['scroll'])
    for op in spec['loads']:
        v.apply(op)
        if op.get('map_add'):
            _map_add(v.mem[op['mem']], op['at'], op['size'], *op['map_add'])
    if residual:
        for k, runs in spec.get('residual', {}).items():
            for s, h in runs:
                b = np.frombuffer(bytes.fromhex(h), np.uint8)
                v.mem[k][s:s + len(b)] = b
    owned = {k: np.zeros(len(m), bool) for k, m in v.mem.items()}
    tp = np.frombuffer(text_palette(), np.uint8)
    for eng in spec.get('text_palette', ()):
        o = 15 * 32 if eng == 'main' else 0x400 + 15 * 32
        v.mem['pal'][o:o + 32] = tp; owned['pal'][o:o + 32] = True
    for c in spec['canvases']:
        cs = canvases.CanvasSpec(_Box(c), [tuple(d) for d in c['draws']])
        before = {k: v.mem[k].copy() for k in ('bg_main', 'bg_sub')}
        cs.apply(v, c['colour'])
        m, a, b = cs.tile_range()
        owned[m][a:b] = True
        owned[m] |= v.mem[m] != before[m]
    return v, owned


def vram(spec, residual=True):
    """the Vram model of a spec (files + residual + text palette + canvases + sprites; nothing read from RAM)"""
    v, owned = _compose(spec, residual)
    sc = _Scroll(spec)
    v.scroll_override = {(e, n): sc(e, n) for e in ('main', 'sub') for n in range(4)}
    return v


def render(spec):
    return vram(spec).render()
