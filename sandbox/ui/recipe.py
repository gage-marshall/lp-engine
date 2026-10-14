"""Screen recipes: the load operations (from a replay trace) whose bytes are still in VRAM at a capture, and a rebuild of the
capture's VRAM from the game's files alone (resolved loads), measured byte for byte against the captured VRAM.

Targets (`GX(S)_Load*` offsets are relative to the target; NNS VRAM-transfer destination types per NNSGfdVramTransferTaskType):
  bg char/scr: relative to the BG's char/screen base (BGxCNT, + DISPCNT bases on the main engine)
  obj: OBJ VRAM; pal: palette RAM (main BG 0x000, main OBJ 0x200, sub BG 0x400, sub OBJ 0x600)
    .venv/bin/python sandbox/ui/recipe.py TRACE_DIR [capture-frame ...]"""
import glob, json, os, sys
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ds2d, provenance  # noqa: E402

# NNS_GFD_DST_* (gfd_VramTransferManager.h)
XFER = {4: 'main_bg0_char', 5: 'main_bg1_char', 6: 'main_bg2_char', 7: 'main_bg3_char', 8: 'main_bg0_scr', 9: 'main_bg1_scr',
        10: 'main_bg2_scr', 11: 'main_bg3_scr', 14: 'main_obj_pal', 15: 'main_bg_pal', 19: 'main_obj',
        20: 'sub_bg0_char', 21: 'sub_bg1_char', 22: 'sub_bg2_char', 23: 'sub_bg3_char', 24: 'sub_bg0_scr', 25: 'sub_bg1_scr',
        26: 'sub_bg2_scr', 27: 'sub_bg3_scr', 30: 'sub_obj_pal', 31: 'sub_bg_pal', 35: 'sub_obj'}
PAL = {'main_bg_pal': 0x000, 'main_obj_pal': 0x200, 'sub_bg_pal': 0x400, 'sub_obj_pal': 0x600}

_FILES = {}


def file_bytes(arc, entry):
    if arc and entry == -1:
        from lpui.vram import file_bytes as fb
        return fb(arc, entry)
    key = (arc, entry)
    if key not in _FILES:
        root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        fs = glob.glob(os.path.join(root, 'content/assets', arc or '', '%04d.*' % entry)) if arc else []
        _FILES[key] = open(fs[0], 'rb').read() if fs else None
    return _FILES[key]


def target_region(cap, target, off):
    """(memory name, absolute offset) of a load target in the capture's memory dumps"""
    if target in PAL:
        return 'pal', PAL[target] + off
    eng, rest = target.split('_', 1)
    if rest == 'obj':
        return 'obj_' + eng, off
    n, kind = int(rest[2]), rest.split('_')[1]
    dc = cap.reg(eng, 0, 4)
    cnt = cap.reg(eng, 8 + 2 * n)
    if kind == 'char':
        base = (cnt >> 2 & 15) * 0x4000 + ((dc >> 24 & 7) * 0x10000 if eng == 'main' else 0)
    else:
        base = (cnt >> 8 & 31) * 0x800 + ((dc >> 27 & 7) * 0x10000 if eng == 'main' else 0)
    return 'bg_' + eng, base + off


def ops_until(trace_dir):
    """yield (capture path, [load ops in order]) per capture; op = dict(mem, at, size, origin, zero) resolved online by
    replay_trace (absolute VRAM target from the registers at load time)"""
    ops = []
    for line in open(os.path.join(trace_dir, 'trace.jsonl')):
        if '"load"' not in line and '"capture"' not in line:
            continue
        r = json.loads(line)
        if r['ev'] == 'load':
            ops.append(r)
        elif r['ev'] == 'capture':
            yield r['path'], list(ops)


def rebuild(cap, ops):
    """VRAM rebuilt from the files (and known zero fills): {memory: bytes}, written mask, reproduced mask"""
    mem = {k: np.zeros(len(v), np.uint8) for k, v in cap.vram.items()}
    mem['pal'] = np.zeros(0x800, np.uint8)
    wr = {k: np.zeros(len(v), bool) for k, v in mem.items()}
    ff = {k: np.zeros(len(v), bool) for k, v in mem.items()}
    vc = bytes(cap.regs['vramcnt']).hex()
    for op in ops:
        if op.get('vc') not in (None, vc):
            continue                                     # loaded under another bank mapping: not what this capture shows
        name, a = op['mem'], op['at']
        n = min(op['size'], len(mem[name]) - a)
        if n <= 0 or a < 0:
            continue
        wr[name][a:a + n] = True
        o = op['origin']
        data = file_bytes(o.get('arc'), o.get('entry', -1)) if o and not o.get('packed') else None
        if data is not None and o['off'] + n <= len(data):
            mem[name][a:a + n] = np.frombuffer(data[o['off']:o['off'] + n], np.uint8)
            ff[name][a:a + n] = True
        elif op.get('zero'):
            mem[name][a:a + n] = 0
            ff[name][a:a + n] = True
        else:
            ff[name][a:a + n] = False
    return mem, wr, ff


def check(trace_dir, frames=None):
    rows = []
    for path, ops in ops_until(trace_dir):
        f = int(path.rsplit('-f', 1)[1])
        if frames and f not in frames:
            continue
        cap = ds2d.Capture(path)
        mem, wr, ff = rebuild(cap, ops)
        actual = {k: np.frombuffer(v, np.uint8) for k, v in cap.vram.items()}
        actual['pal'] = cap.pal.view(np.uint8)
        stats = {}
        for k in mem:
            eq = (mem[k] == actual[k]) & ff[k]
            stats[k] = (int(ff[k].sum()), int(eq.sum()), int((wr[k] & ~ff[k]).sum()))
        rows.append((f, stats))
    return rows


if __name__ == '__main__':
    frames = {int(x) for x in sys.argv[2:]} or None
    tot = [0, 0, 0]
    for f, st in check(sys.argv[1], frames):
        s = ' '.join('%s %d/%d gen %d' % (k, v[1], v[0], v[2]) for k, v in st.items())
        for v in st.values():
            tot[0] += v[0]; tot[1] += v[1]; tot[2] += v[2]
        print(f, s)
    print('from files: %d bytes, %d equal to the capture (%.3f%%); generated: %d bytes' % (tot[0], tot[1], 100 * tot[1] / max(1, tot[0]), tot[2]))
