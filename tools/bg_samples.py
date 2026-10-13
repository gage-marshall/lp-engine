#!/usr/bin/env python3
"""Tabulate (event, game date/hour, tmx_bbin background id) against the BG archive entry the game really loaded (play_monitor res events)."""
import json, glob, struct, sys
recs = {}
for f in sorted(glob.glob('content/assets/arc/tmx_bbin.arc/*.bin')):
    b = open(f, 'rb').read(); n = struct.unpack_from('<I', b, 0)[0]
    for i in range(n):
        ev, bg, x, y = struct.unpack_from('<IHBB', b, 4 + 8 * i); recs.setdefault(ev, []).append((f[-8:-4], bg, x, y))
seen = set()
for lf in sorted(glob.glob('tools/emu_scripts/states/logs/session-*.jsonl')):
    ev = [json.loads(l) for l in open(lf)]
    for i, e in enumerate(ev):
        if e['ev'] != 'script_start': continue
        sid = int(e['id'], 16) & 0xffffffff
        for k in range(max(0, i - 60), min(len(ev), i + 20)):
            x = ev[k]
            if x['ev'] == 'res' and x['fn'] == 'ResTable_Open' and x['strs'] and x['strs'][0].startswith('arc/bg_') and 'bg_scroll' not in x['strs'][0]:
                ent = next((int(y['args'][1], 16) for y in ev[k + 1:k + 8] if y['ev'] == 'res' and y['fn'] == 'ResTable_ReadEntry'), None)
                row = (e.get('date'), e.get('hour'), '%08x' % sid, [r[1:3] for r in recs.get(sid, [])], x['strs'][0][4:-4], ent // 3 if ent is not None else None)
                if str(row) not in seen: seen.add(str(row)); print(row)
                break
