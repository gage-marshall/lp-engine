#!/usr/bin/env python3
"""Export the decoded event-selection data to content/engine/event_selection.json (ROM-derived, local, git-ignored).

Usage: event_export.py ARM9_MAIN_BIN TMX_TBIN_DIR OUT.json

Contents:
  stages        the 36-stage resolver table at 0x210b8f0: [{mask, kind, handler}] in priority order (MicSession_Clear)
  text_routes   SceneRoute_SelectTextRoute tables (tools/event_routes.py)
  conditions    every TERM_BIN record of tmx_tbin.arc: {file, id, priority, flag, groups: [[[op,a,b]...] x3]}
  cond_handlers the op -> handler-address table at 0x21246c8 (237 entries; 0 = none)
"""
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from event_routes import dump as dump_routes  # noqa: E402
from term import parse  # noqa: E402

BASE = 0x02000000


def main():
    arm9 = open(sys.argv[1], 'rb').read()

    def r32(a):
        return struct.unpack_from('<I', arm9, a - BASE)[0]
    tab = r32(0x20c1fc0)
    stages = [{'mask': r32(tab + i * 12), 'kind': r32(tab + i * 12 + 4) & 0xffff, 'handler': r32(tab + i * 12 + 8) & ~1}
              for i in range(36)]
    routes = dump_routes(arm9)
    handlers = [r32(0x21246c8 + 4 * i) for i in range(237)]
    conds = []
    tdir = sys.argv[2]
    for f in sorted(os.listdir(tdir)):
        recs, _ = parse(open(os.path.join(tdir, f), 'rb').read())
        for r in recs:
            conds.append({'file': f, 'id': r['id'], 'priority': r['priority'], 'flag': r['flag'], 'groups': r['groups']})
    out = {'stages': stages, 'text_routes': {'default': routes['default'], 'kinds': {str(k): v for k, v in routes['kinds'].items()}},
           'conditions': conds, 'cond_handlers': handlers}
    os.makedirs(os.path.dirname(sys.argv[3]), exist_ok=True)
    json.dump(out, open(sys.argv[3], 'w'))
    print('stages', len(stages), 'records', len(conds), 'handlers', sum(1 for h in handlers if h))


if __name__ == '__main__':
    main()
