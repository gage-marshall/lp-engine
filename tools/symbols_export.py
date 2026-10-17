#!/usr/bin/env python3
"""The shareable symbol map: every function of LovePlus+ (BOXJ) we have named, with where it is, how big, how sure we
are of the name, a short note, and the direct calls between them. Only addresses, sizes and our own names and notes:
no bytes of the game or the SDK, no FID databases, no decompiled code.

    symbols_export.py CONTENT OUT_DIR

writes OUT_DIR/loveplus_plus_boxj.csv (module, address, size, name, kind, confidence, note) and
OUT_DIR/loveplus_plus_boxj.json (the same plus the call graph, for the site's explorer). The call graph is a static
scan of the direct calls (BL / BLX) in the player's own extracted code (CONTENT/engine/*.bin); only calls landing on
a named function's first instruction are kept, and calls through pointers are not seen.
"""
import csv
import glob
import json
import os
import re
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FID = os.path.join(ROOT, 'fid')

# where a name came from -> what the explorer shows
KIND = {'sdk': 'sdk', 'nns': 'nitrosystem', 'sem': 'game', 'manual': 'game', 'runtime': 'library'}
CONF = {'high': 'high', 'med': 'medium', 'low': 'low'}


def kind_of(r):
    src = r['source'].split('/')[0]
    return KIND.get(src, 'game')


def confidence_of(r, kind):
    # library functions matched against the libraries themselves are exact; hand-named ones (manual) were verified
    if kind in ('sdk', 'nitrosystem'):
        return 'exact'
    # the libraries' internal helpers, placed by relocations and a donor project's matches
    if kind == 'library':
        return 'high'
    if r['source'] == 'manual':
        return 'verified'
    return CONF.get(r['confidence'], 'low')


def sizes():
    out = {}
    for line in open(os.path.join(FID, 'funcs_final.txt')):
        a, s, *_ = line.split()
        out[('static', int(a, 16))] = int(s)
    for f in glob.glob(os.path.join(FID, 'auto', 'ov*.txt')):
        mod = 'ov%02d' % int(re.search(r'ov(\d+)', f).group(1))
        for line in open(f):
            p = line.split()
            if len(p) >= 3 and re.fullmatch(r'[0-9a-f]{8}', p[0]) and p[2].isdigit():
                out[(mod, int(p[0], 16))] = int(p[2])
    return out


def calls(content, funcs):
    """direct calls: (module, caller start) -> set of (module, callee start)"""
    mm = json.load(open(os.path.join(content, 'engine', 'memory_map.json')))
    starts = {}
    for (mod, a) in funcs:
        starts.setdefault(a, []).append(mod)
    by_mod = {}
    for (mod, a), s in funcs.items():
        if s:
            by_mod.setdefault(mod, []).append((a, s))
    edges = {}
    for mod, fl in by_mod.items():
        name = 'static' if mod == 'static' else mod
        path = os.path.join(content, 'engine', name + '.bin')
        info = mm.get(name)
        if not info or not os.path.isfile(path):
            continue
        b = open(path, 'rb').read()
        base = info['base']
        # the static image holds ITCM code at its own addresses (below 0x2000000): read it from itcm.bin
        itcm = open(os.path.join(content, 'engine', 'itcm.bin'), 'rb').read() if mod == 'static' else b''
        itcm_base = mm['itcm']['base'] if mod == 'static' else 0

        def resolve(t):
            mods = starts.get(t)
            if not mods:
                return None
            if t < 0x2158000 or 'static' in mods:
                return ('static', t) if 'static' in mods else None
            if mod in mods:
                return (mod, t)
            return (mods[0], t) if len(mods) == 1 else None

        for a, s in fl:
            if a < 0x2000000:
                img, off = itcm, a - itcm_base
            else:
                img, off = b, a - base
            if off < 0 or off + s > len(img):
                continue
            seen = set()
            for o in range(off, off + s - 3, 2):
                pc = (a - off) + o
                w = struct.unpack_from('<I', img, o)[0]
                t = None
                if o % 4 == 0 and (w >> 25) & 7 == 5 and (w >> 24) & 1 == 1 and (w >> 28) != 0xf:  # ARM BL
                    d = w & 0xffffff
                    t = pc + 8 + ((d - (1 << 24)) if d & 0x800000 else d) * 4
                elif o % 4 == 0 and (w >> 25) == 0x7d:  # ARM BLX imm
                    d = w & 0xffffff
                    t = pc + 8 + ((d - (1 << 24)) if d & 0x800000 else d) * 4 + ((w >> 24) & 1) * 2
                else:
                    h1, h2 = w & 0xffff, w >> 16
                    if h1 >> 11 == 0x1e and h2 >> 11 in (0x1f, 0x1d):  # Thumb BL / BLX pair
                        d = ((h1 & 0x7ff) << 12) | ((h2 & 0x7ff) << 1)
                        if d & 0x400000:
                            d -= 1 << 23
                        t = pc + 4 + d
                        if h2 >> 11 == 0x1d:
                            t &= ~3
                if t is not None:
                    r = resolve(t)
                    if r and r != (mod, a):
                        seen.add(r)
            if seen:
                edges[(mod, a)] = seen
    return edges


# corrected names read from the code (docs/GAME_FLOW.md), beyond those the flow port carries (engine/src/flow/addr.rs)
CORRECTED = {
    # the script manager and the event pipeline (docs/SCRIPT_VM.md section 9, docs/EVENT_SELECTION.md)
    ('static', 0x205e7c4): ('ScriptMgr_Start', 'verified'),
    ('static', 0x205e5a0): ('ScriptMgr_Update', 'verified'),
    ('static', 0x205a8f4): 'Script_DispatchCommand',
    ('static', 0x20c1f24): ('EventSession_RunStages', 'verified'),
    ('static', 0x20c1fc4): 'EventSession_Process',
    ('static', 0x20c1dd0): 'EventSession_BuildPool',
    ('static', 0x20c237c): 'EventCond_CheckEligible',
    ('static', 0x20c21ac): 'EventCond_BuildContextWord',
    # the save writer, proven byte for byte against our card writer (docs/SAVE_FORMAT.md)
    ('static', 0x20f0fb8): ('Save_WriteBlock', 'verified'),
    ('static', 0x20f0d64): 'Save_GetCopyTableIndex',
    ('static', 0x20882c8): 'Screen_SwitchTo',
    ('static', 0x2088624): 'Screen_PostResult',
    ('static', 0x2088644): 'Screen_FinishWithAltResult',
    ('static', 0x208a1f0): 'Screen_GetEntryArg',
    ('static', 0x208a200): 'Screen_SetEntryArg',
}


def corrections():
    """(module, address) -> (corrected name, confidence): the flow port's corrected labels (checked against 311,719
    frames of play) and the ones read from the code"""
    out = {}
    for line in open(os.path.join(ROOT, 'engine', 'src', 'flow', 'addr.rs')):
        m = re.match(r'pub const (\w+): u32 = 0x([0-9a-f]+);\s*// labelled (\w+)', line)
        if m:
            out[('static', int(m.group(2), 16))] = (m.group(1), 'verified')
    for k, n in CORRECTED.items():
        out.setdefault(k, n if isinstance(n, tuple) else (n, 'high'))
    return out


# address blocks whose labels share a wrong reading: (module, from, to, {old prefix: new prefix}, note, confidence or
# None to keep). Prefixes not in the map keep their name; the note still applies.
BLOCKS = [
    ('static', 0x20c0430, 0x20c3300, {'MicSession_': 'EventSession_', 'MicSequence_': 'EventCond_'},
     'not the microphone: this block is the event condition engine (docs/EVENT_SELECTION.md)', None),
    ('static', 0x20c0430, 0x20c3300, {},
     'the "Mic" in this name is wrong: this block is the event condition engine (docs/EVENT_SELECTION.md); what this '
     'function does in it is unconfirmed', 'low'),
    ('static', 0x20ed790, 0x20ef700, {'OverlayState_': 'FashionState_', 'Overlay_': 'Fashion_'},
     'not overlays: this block loads the fashion (outfit) parameter tables (docs/EVENT_SELECTION.md)', None),
]


def fix_block(mod, a, name):
    """a name corrected by its block: (name, note, confidence or None), or None"""
    for bm, lo, hi, prefixes, note, conf in BLOCKS:
        if bm != mod or not lo <= a < hi:
            continue
        for old, new in prefixes.items():
            if name.startswith(old):
                return new + name[len(old):], 'formerly labelled %s; %s' % (name, note), conf
        if not prefixes and name.startswith('Mic'):
            return name, note, conf
    return None


# functions whose behaviour was confirmed by disassembly and in the engine (name kept): (module, address) -> note
VERIFIED = {
    ('static', 0x2045a68): 'Once per frame: counts the frames the lid has been closed (bit 15 of the shared key word); '
                           'after 8 it puts the DS to sleep with PM_GoSleepMode, otherwise it handles the backlight '
                           '(PM_SetLCDPower). Returns 3 after a sleep or a backlight change (Main_UpdateFrame then flags the frame), '
                           'else 0, or 2 while a backlight key toggle is pending.',
}


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    content, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    rows = list(csv.DictReader(open(os.path.join(FID, 'labels_all.csv'))))
    sz = sizes()
    fixed = corrections()
    syms = []
    for r in rows:
        mod, a = r['module'], int(r['addr'], 16)
        k = kind_of(r)
        name, conf, note = r['name'], confidence_of(r, k), r['note']
        if (mod, a) in VERIFIED:
            conf, note = 'verified', VERIFIED[(mod, a)]
        if (mod, a) in fixed:
            name, conf = fixed[(mod, a)]
            # the old note described the old reading
            note = 'formerly labelled %s' % r['name']
        else:
            b = fix_block(mod, a, name)
            if b:
                name, extra, c = b
                note = (extra + ('. ' + note if note else '')).strip()
                conf = c or conf
        syms.append({'m': mod, 'a': a, 's': sz.get((mod, a)), 'n': name, 'k': k, 'c': conf, 'note': note.strip()})
    syms.sort(key=lambda x: (x['m'] != 'static', x['m'], x['a']))
    with open(os.path.join(out, 'loveplus_plus_boxj.csv'), 'w', newline='') as f:
        w = csv.writer(f)
        w.writerow(['module', 'address', 'size', 'name', 'kind', 'confidence', 'note'])
        for x in syms:
            w.writerow([x['m'], '%08x' % x['a'], x['s'] if x['s'] is not None else '', x['n'], x['k'], x['c'], x['note']])
    funcs = {(x['m'], x['a']): x['s'] for x in syms}
    index = {(x['m'], x['a']): i for i, x in enumerate(syms)}
    edges = calls(content, funcs)
    out_calls = [[] for _ in syms]
    for src, dsts in edges.items():
        out_calls[index[src]] = sorted(index[d] for d in dsts if d in index)
    data = {'game': 'LovePlus+ (BOXJ)', 'fields': ['module', 'address', 'size', 'name', 'kind', 'confidence', 'note'],
            'symbols': [[x['m'], x['a'], x['s'], x['n'], x['k'], x['c'], x['note']] for x in syms], 'calls': out_calls}
    json.dump(data, open(os.path.join(out, 'loveplus_plus_boxj.json'), 'w'), separators=(',', ':'))
    from collections import Counter
    print('%d symbols, %d with calls, %d call edges' % (len(syms), sum(1 for c in out_calls if c), sum(len(c) for c in out_calls)))
    print(Counter(x['k'] for x in syms), Counter(x['c'] for x in syms))


if __name__ == '__main__':
    main()
