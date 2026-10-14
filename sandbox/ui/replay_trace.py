"""Replay a recorded play_monitor session headlessly (same start state, pinned RTC, logged inputs) and trace how every screen
is built: resource reads (archive, entry -> buffer), LZ decompression (buffer -> buffer), VRAM loads (GX/GXS_Load*, deferred
VramTransfer_Register: buffer -> VRAM), overlay loads, hit-list registrations; full display captures at the session's capture
frames, checked against the session's screenshots (replay fidelity).
    .venv/bin/python sandbox/ui/replay_trace.py SESSION.jsonl START_STATE OUT_DIR [--until FRAME]"""
import json, os, sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools')); sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from emu_driver import Emu  # noqa: E402
import capture  # noqa: E402
from bufmap import BufMap  # noqa: E402
import glob  # noqa: E402
import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

GX = {0x201b4c4: 'main_bg_pal', 0x201b528: 'sub_bg_pal', 0x201b584: 'main_obj_pal', 0x201b5e0: 'sub_obj_pal',
      0x201b63c: 'main_oam', 0x201b6a0: 'sub_oam', 0x201b6fc: 'main_obj', 0x201b760: 'sub_obj',
      0x201b7c4: 'main_bg0_scr', 0x201b834: 'sub_bg0_scr', 0x201b8a4: 'main_bg1_scr', 0x201b914: 'sub_bg1_scr',
      0x201b984: 'main_bg2_scr', 0x201b9f4: 'sub_bg2_scr', 0x201ba64: 'main_bg3_scr', 0x201bad4: 'sub_bg3_scr',
      0x201bb44: 'main_bg0_char', 0x201bbb4: 'sub_bg0_char', 0x201bc24: 'main_bg1_char', 0x201bc94: 'sub_bg1_char',
      0x201bd04: 'main_bg2_char', 0x201bd74: 'sub_bg2_char', 0x201bde4: 'main_bg3_char', 0x201be54: 'sub_bg3_char',
      0x201bf50: 'main_bg_extpal', 0x201c04c: 'main_obj_extpal', 0x201c110: 'sub_bg_extpal', 0x201c1c8: 'sub_obj_extpal'}
RES_OPEN, RES_READ, RES_LOAD = 0x204e644, 0x204e890, 0x204e838
LZ8, VRAMXFER, OVL, HIT = 0x201ef8c, 0x20450f8, 0x20ef978, 0x20839a4


def main():
    sess, state, out = sys.argv[1], sys.argv[2], sys.argv[3]
    until = int(sys.argv[sys.argv.index('--until') + 1]) if '--until' in sys.argv else None
    recs = [json.loads(l) for l in open(sess)]
    inputs = {}
    for r in recs:
        if r['ev'] == 'in':
            inputs[r['f']] = (r['mask'], r['touch'])
    cap_frames = {r['f']: r['path'] for r in recs if r['ev'] == 'ui_capture'}
    end = until or max(r['f'] for r in recs)
    os.makedirs(out, exist_ok=True)
    e = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), state)
    e.pin_rtc()
    m = e.e.memory
    reg = m.register_arm9
    log = open(os.path.join(out, 'trace.jsonl'), 'w')
    tables = {}

    def w(ev, **kw):
        kw.update(f=e.frames, ev=ev)
        if ev == 'load':
            kw['vc'] = bytes(m.unsigned[0x4000240 + i] for i in range(10)).hex()   # VRAM bank mapping at load time
        log.write(json.dumps(kw) + '\n')

    def rstr(a, n=64):
        try:
            b = bytes(m.unsigned[a + i] for i in range(n)); return b.split(b'\0')[0].decode('latin1')
        except Exception:
            return None

    bm = BufMap()
    returns = {}            # return address -> [callback]
    ret_hooked = set()

    def at_return(fn):
        ret = reg.lr & ~1
        returns.setdefault(ret, []).append(fn)
        if ret not in ret_hooked:
            ret_hooked.add(ret)
            m.register_exec(ret, lambda addr, size, ret=ret: returns[ret].pop()() if returns.get(ret) else None)

    tables = {}
    sizes = {}

    def fsize(arc, entry):
        k = (arc, entry)
        if k not in sizes:
            fs = glob.glob(os.path.join(ROOT, 'content/assets', arc or '-', '%04d.*' % entry))
            sizes[k] = os.path.getsize(fs[0]) if fs else 0x10000
        return sizes[k]

    def rd32(a):
        return m.unsigned.read_long(a) & 0xffffffff

    def vram_target(name, off):
        """absolute target of a load: ('bg_main'|'bg_sub'|'obj_main'|'obj_sub'|'pal', offset), from the registers now"""
        if name.endswith('_pal'):
            return 'pal', {'main_bg_pal': 0, 'main_obj_pal': 0x200, 'sub_bg_pal': 0x400, 'sub_obj_pal': 0x600}[name] + off
        eng, rest = name.split('_', 1)
        if rest == 'obj':
            return 'obj_' + eng, off
        n, kind = int(rest[2]), rest.split('_')[1]
        io = 0x4000000 if eng == 'main' else 0x4001000
        dc = rd32(io)
        cnt = (rd32(io + 8 + 4 * (n >> 1)) >> (16 * (n & 1))) & 0xffff
        if kind == 'char':
            base = (cnt >> 2 & 15) * 0x4000 + ((dc >> 24 & 7) * 0x10000 if eng == 'main' else 0)
        else:
            base = (cnt >> 8 & 31) * 0x800 + ((dc >> 27 & 7) * 0x10000 if eng == 'main' else 0)
        return 'bg_' + eng, base + off

    def load(name, src, off, size, lr, how):
        if name.endswith('oam') or 'extpal' in name:
            return
        mem, a = vram_target(name, off)
        o = bm.origin(src)
        zero = o is None and size <= 0x2000 and all(m.unsigned[src + i] == 0 for i in range(0, size, max(1, size // 16)))
        w('load', mem=mem, at=a, size=size, origin=o, zero=bool(zero), lr=lr, how=how, target=name)

    def on_gx(name):
        return lambda a, s: load(name, reg.r0, reg.r1, reg.r2, reg.lr, 'gx')
    for a, n in GX.items():
        m.register_exec(a, on_gx(n))

    def on_open(a, s):
        tables[reg.r0] = rstr(reg.r1); w('res_open', slot=reg.r0, name=tables[reg.r0])
    m.register_exec(RES_OPEN, on_open)

    def on_read(a, s):
        arc, entry, dest, size = tables.get(reg.r0), reg.r1, reg.r2, reg.r3

        def done():                                          # the buffer holds the entry once the call returns
            bm.put(dest, size, dict(arc=arc, entry=entry))
            w('res_read', arc=arc, entry=entry, dest=dest, size=size)
        at_return(done)
    m.register_exec(RES_READ, on_read)

    # file-system reads (mounted archives are read whole and used in place): locate the bytes in the raw file and split
    # the buffer by the archive's entry table
    fsfiles = {}
    raws = {}

    def raw(path):
        if path not in raws:
            fp = os.path.join(ROOT, 'work/fs', path)
            d = open(fp, 'rb').read() if os.path.isfile(fp) else None
            ents = []
            if d and len(d) > 16 and int.from_bytes(d[:4], 'little') == 0x15618:
                cnt = int.from_bytes(d[4:6], 'little'); first = int.from_bytes(d[12:16], 'little')
                stride = (first - 12) // cnt if cnt else 12
                for i in range(cnt):
                    o, sz, u = (int.from_bytes(d[12 + i * stride + 4 * k:16 + i * stride + 4 * k], 'little') for k in range(3))
                    ents.append((i, o, sz, u))
            raws[path] = (d, ents)
        return raws[path]

    def on_fsopen(a, s):
        fsfiles[reg.r0] = rstr(reg.r1, 96)
    m.register_exec(0x2016b80, on_fsopen)

    def on_fsread(a, s):
        f, dst, n = reg.r0, reg.r1, reg.r2

        def done():
            path = fsfiles.get(f)
            if not path or n <= 0:
                return
            d, ents = raw(path.lstrip('/'))
            if d is None:
                return
            head = bytes(m.unsigned[dst:dst + min(n, 64)])
            off = d.find(head)
            if off < 0:
                return
            w('fs_read', path=path, dst=dst, size=n, off=off)
            if not ents:
                bm.put(dst, n, dict(arc=path.lstrip('/'), entry=-1, off=off)); return
            bm.put(dst, n, None)
            for i, o, sz, u in ents:
                lo, hi = max(o, off), min(o + sz, off + n)
                if lo < hi:
                    bm.put(dst + lo - off, hi - lo, dict(arc=path.lstrip('/'), entry=i, off=lo - o, packed=u != 0 and sz != u))
        at_return(done)
    m.register_exec(0x2016d20, on_fsread)
    def on_resload(a, s):
        slot, entry = reg.r0, reg.r1

        def done():
            arc = tables.get(slot)
            bm.put(reg.r0, fsize(arc, entry), dict(arc=arc, entry=entry))
            w('res_loaded', arc=arc, entry=entry, ptr=reg.r0)
        at_return(done)
    m.register_exec(RES_LOAD, on_resload)

    def vram_mem(addr):
        for base, size, name in ((0x6000000, 0x80000, 'bg_main'), (0x6200000, 0x20000, 'bg_sub'), (0x6400000, 0x40000, 'obj_main'),
                                 (0x6600000, 0x20000, 'obj_sub'), (0x5000000, 0x800, 'pal')):
            if base <= addr < base + size:
                return name, addr - base
        return None

    def direct(dst, n, origin, zero, how):
        v = vram_mem(dst)
        if v and n > 0:
            w('load', mem=v[0], at=v[1], size=n, origin=origin, zero=zero, lr=reg.lr, how=how, target=how)

    def on_copy(a, s):
        n = reg.r2
        if n > 0 and vram_mem(reg.r1):
            direct(reg.r1, n, bm.origin(reg.r0), False, 'cpu_copy')
        if n > 0:
            hit = bm.find(reg.r0)
            if hit:
                st, en, o = hit
                bm.put(reg.r1, n, dict(o, off=o.get('off', 0) + (reg.r0 - st)))
            else:
                bm.put(reg.r1, n, None)
    for addr in (0x201eb54, 0x201e88c, 0x201e8bc, 0x201e938, 0x201ec84):
        m.register_exec(addr, on_copy)

    def on_lz(a, s):
        o = bm.origin(reg.r0)
        n = rd32(reg.r0) >> 8
        bm.put(reg.r1, n, dict(arc=o['arc'], entry=o['entry'], lz=True, off=0) if o else None)
    m.register_exec(LZ8, on_lz)

    def on_lz16(a, s):
        o = bm.origin(reg.r0)
        n = rd32(reg.r0) >> 8
        o2 = dict(arc=o['arc'], entry=o['entry'], lz=True, off=0) if o else None
        if vram_mem(reg.r1):
            direct(reg.r1, n, o2, False, 'lz16')
        bm.put(reg.r1, n, o2)
    m.register_exec(0x201f06c, on_lz16)

    def on_clear(a, s):                                   # MIi_CpuClear16/32/Fast(value, dest, size)
        canvas_write(reg.r1, reg.r2, reg.r0, 'clear')
        direct(reg.r1, reg.r2, None, reg.r0 == 0, 'clear')
        bm.put(reg.r1, reg.r2, None)
    for addr in (0x201e874, 0x201e8a8, 0x201e8ec):
        m.register_exec(addr, on_clear)

    def on_fill(a, s):                                    # MI_CpuFill8 / MI_CpuFill(dest, value, size)
        canvas_write(reg.r0, reg.r2, reg.r1 & 0xff, 'fill8')
        direct(reg.r0, reg.r2, None, (reg.r1 & 0xff) == 0, 'fill')
        bm.put(reg.r0, reg.r2, None)
    for addr in (0x201e9dc, 0x201ea70):
        m.register_exec(addr, on_fill)
    XF = {4: 'main_bg0_char', 5: 'main_bg1_char', 6: 'main_bg2_char', 7: 'main_bg3_char', 8: 'main_bg0_scr', 9: 'main_bg1_scr',
          10: 'main_bg2_scr', 11: 'main_bg3_scr', 14: 'main_obj_pal', 15: 'main_bg_pal', 19: 'main_obj', 20: 'sub_bg0_char',
          21: 'sub_bg1_char', 22: 'sub_bg2_char', 23: 'sub_bg3_char', 24: 'sub_bg0_scr', 25: 'sub_bg1_scr', 26: 'sub_bg2_scr',
          27: 'sub_bg3_scr', 30: 'sub_obj_pal', 31: 'sub_bg_pal', 35: 'sub_obj'}
    m.register_exec(VRAMXFER, lambda a, s: w('vram_xfer', type=reg.r0, dst=reg.r1, src=reg.r2, size=reg.r3, origin=bm.origin(reg.r2), lr=reg.lr))
    last_hits = {}

    def on_poll(a, s):
        """TouchCtl_PollPress(out, ctl): ctl+4i = bound hit list i (0..4); list: items {x, y, halfW, halfH, id, enabled} x 0x18,
        count at +0xc04 - log the active touch areas whenever they change"""
        ctl = reg.r1
        lists = []
        for i in range(5):
            l = rd32(ctl + 4 * i)
            if not 0x2000000 <= l < 0x2400000:
                lists.append(None); continue
            n = rd32(l + 0xc04)
            if n > 64:
                lists.append(None); continue
            items = []
            for k in range(n):
                v = [rd32(l + 0x18 * k + 4 * j) for j in range(6)]
                items.append([x - (1 << 32) if x >= 1 << 31 else x for x in v])
            lists.append(items)
        key = json.dumps(lists)
        if last_hits.get('pf') != e.frames:                # one poll record per frame and control
            last_hits['pf'] = e.frames
            last_hits['pc'] = set()
        if ctl not in last_hits['pc']:
            last_hits['pc'].add(ctl)
            log.write('{"ev":"poll","f":%d,"ctl":%d}\n' % (e.frames, ctl))
        if last_hits.get(ctl) != key:
            last_hits[ctl] = key
            w('hits', ctl=ctl, lists=lists, lr=reg.lr)
    m.register_exec(0x2083838, on_poll)
    m.register_exec(0x20838c4, on_poll)              # TouchCtl_PollHold: same control layout
    # write-only BG scroll registers: mirror every write (the hook gives the address; the value comes from the source
    # register of the store instruction at PC: STR / STRH / STM)
    scroll = {}

    def store_value(addr, size):
        pc = reg.pc
        for at in (pc, pc - 4, pc - 8):
            ins = rd32(at)
            if (ins >> 26) & 3 == 1 and not (ins >> 20) & 1:                       # STR / STRB
                return getattr(reg, 'r%d' % ((ins >> 12) & 15))
            if (ins >> 25) & 7 == 0 and (ins & 0xf0) == 0xb0 and not (ins >> 20) & 1:  # STRH
                return getattr(reg, 'r%d' % ((ins >> 12) & 15))
            if (ins >> 25) & 7 == 4 and not (ins >> 20) & 1:                       # STM: first register for the lowest address
                regs = [i for i in range(16) if ins >> i & 1]
                base = getattr(reg, 'r%d' % ((ins >> 16) & 15))
                k = (addr - base) // 4 if addr >= base else 0
                if 0 <= k < len(regs):
                    return getattr(reg, 'r%d' % regs[k])
        return None

    def on_scroll(addr, size):
        v = store_value(addr, size)
        if v is None:
            return
        for k in range(size // 2 if size >= 2 else 1):
            a = addr + 2 * k
            scroll['%x' % a] = (v >> (16 * k)) & (0x1ff if (a & 0xfff) < 0x20 else 0xffff)
    for base in (0x4000010, 0x4001010):
        for o in range(0, 16, 2):
            m.register_write(base + o, on_scroll, 2)
    for a in (0x4000054, 0x4001054, 0x400006c, 0x400106c):
        m.register_write(a, on_scroll, 2)
    shown = {}               # the mirror as it stood when the frame on screen was drawn
    def readable(a):                     # main RAM, or the DTCM stack (one-character copies made on the stack)
        return 0x2000000 <= a < 0x2400000 or 0x27e0000 <= a < 0x27e4000

    def rtext(a):
        if not readable(a):
            return None
        bs = bytes(m.unsigned[a:a + 128]).split(b'\0')[0]
        try:
            t = bs.decode('utf8')
            return t if t and all(ord(c) >= 0x20 or c == '\n' for c in t) else None
        except UnicodeDecodeError:
            return None

    def rraw(a):
        return bytes(m.unsigned[a:a + 128]).split(b'\0')[0].hex() if readable(a) else None

    def on_text(name):
        def cb(a, s):
            args = [reg.r0, reg.r1, reg.r2, reg.r3] + [rd32(reg.sp + 4 * i) for i in range(3)]
            strs = {i: t for i, t in ((i, rtext(v)) for i, v in enumerate(args)) if t}
            raw = rraw(args[1]) if name in ('DrawString', 'SetString') else None
            st = None
            if name == 'DrawNextChar':                    # the text box state the call works from
                o = args[0]
                st = dict(pos=rd32(o + 0x27c), x=rd32(o + 0x274), y=rd32(o + 0x278), y0=rd32(o + 0x290), done=rd32(o + 0x288),
                          s=rraw(o + 0x60), lines=rd32(o + 0x284), n=rd32(o + 0x280))
            w('text', fn=name, args=args, strs=strs, raw=raw, st=st, lr=reg.lr)
        return cb
    canvases = {}            # pCC -> (charBase, size)

    def on_canvas(a, s):
        pcc, base, cw, ch = reg.r0, reg.r1, reg.r2, reg.r3
        canvases[pcc] = (base, cw * ch * 32)
        w('canvas', pcc=pcc, base=base, w=cw, h=ch)
    m.register_exec(0x20131fc, on_canvas)

    def on_glyph(a, s):
        w('glyph', pcc=reg.r0, font=reg.r1, x=reg.r2 - (1 << 32) if reg.r2 >= 1 << 31 else reg.r2,
          y=reg.r3 - (1 << 32) if reg.r3 >= 1 << 31 else reg.r3, cl=rd32(reg.sp), code=rd32(reg.sp + 4) & 0xffff)
    m.register_exec(0x20130e0, on_glyph)
    m.register_exec(0x202cb10, lambda a, s: canvas_write(reg.r0, reg.r2, reg.r1 & 0xff, 'memset'))

    def canvas_write(dst, n, value, how):
        for pcc, (base, size) in canvases.items():
            if base <= dst < base + size:
                w('canvas_fill', pcc=pcc, off=dst - base, size=n, value=value, how=how)
    def on_call(name):
        def cb(a, s):
            w('call', fn=name, args=[reg.r0, reg.r1, reg.r2, reg.r3] + [rd32(reg.sp + 4 * i) for i in range(2)], lr=reg.lr)
        return cb
    for addr, name in ((0x207fe6c, 'DateUi_DrawDay'), (0x207fef4, 'DateUi_DrawMonth'), (0x207ff30, 'DateUi_DrawYear'),
                       (0x207ffb8, 'DateUi_DrawWeekday'), (0x20800c0, 'DateUi_DrawDate'), (0x207f728, 'DateUi_UpdateHeader'),
                       (0x2089788, 'Sprite_SetCellByIdx'), (0x20897e0, 'Sprite_SetAnimSlot'), (0x20895e4, 'Sprite_SetPos'),
                       (0x20491d4, 'Sprite_SetAnim')):
        m.register_exec(addr, on_call(name))
    m.register_exec(0x2083f08, on_text('SetString'))
    m.register_exec(0x2083f48, on_text('SetLayoutParams'))
    m.register_exec(0x2084328, on_text('DrawString'))
    m.register_exec(0x2084530, on_text('DrawNextChar'))
    m.register_exec(0x2084024, on_text('CanvasInitForBG'))
    m.register_exec(OVL, lambda a, s: w('overlay', args=[reg.r0, reg.r1], lr=reg.lr))
    m.register_exec(HIT, lambda a, s: w('hit_add', list=reg.r0, args=[reg.r1, reg.r2, reg.r3] + [rd32(reg.sp + 4 * i) for i in range(3)], lr=reg.lr))
    from desmume.controls import keymask  # noqa: F401
    touching = False
    fid = []
    while e.frames < end:
        f = e.frames
        if f in inputs:
            mask, touch = inputs[f]
            e.e.input.keypad_update(mask)
            if touch:
                e.e.input.touch_set_pos(touch[0], touch[1]); touching = True
            elif touching:
                e.e.input.touch_release(); touching = False
        shown = dict(scroll)
        e.step()
        if e.frames in cap_frames or (e.frames - 1) in cap_frames:
            k = e.frames if e.frames in cap_frames else e.frames - 1
            if k == e.frames:
                d = os.path.join(out, 'cap-f%d' % e.frames)
                capture.capture(e, d)
                json.dump(shown, open(os.path.join(d, 'scroll.json'), 'w'))
                w('capture', path=d)
                ref = os.path.join(cap_frames[k], 'shot.png')
                if os.path.exists(ref):
                    a = np.asarray(Image.open(ref).convert('RGB')).astype(int)
                    b = np.asarray(Image.open(os.path.join(d, 'shot.png')).convert('RGB')).astype(int)
                    fid.append((e.frames, int((np.abs(a - b).max(-1) > 8).sum())))
    log.close()
    json.dump(fid, open(os.path.join(out, 'fidelity.json'), 'w'))
    bad = [x for x in fid if x[1] > 0]
    print('replayed to frame', e.frames, '; captures', len(fid), '; differing from the session:', len(bad), bad[:10])


if __name__ == '__main__':
    main()
