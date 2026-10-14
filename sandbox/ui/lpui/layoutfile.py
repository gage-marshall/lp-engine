"""The game's text screen layouts (menu/mount_layout.bin: 0001 calendar, 0002 profile, 0003 sound settings,
0004 display settings), read by the sprite system (parsed with strtoul; Sprite_UpdateDrawState fills the sprite slot
records, Sprite_Draw places them).

Format (CRLF text, ';' terminators not always present):
  ARC_FILE_PATH:<archive>;                 the screen's archive
  BG_MAIN_RESOURCE_ARC: / BG_SUB_RESOURCE_ARC:  count; then BGn_RESOURCE:<char>,<palette?>,<screen> entries
  OBJ_MAIN_COLOR_PAL_ARC: / OBJ_SUB_COLOR_PAL_ARC: count; then <entry>,<first>,<count> palette loads
  OBJ_MAIN_RESOURCE_ARC: (or OBJ_SUB_...) <groups>; <total sprites>; then per group:
      OBJ_RESOURCE:<cell>,<anim>,<char> (archive entries)  or OBJ_RESOURCE_PATH:<path>
      <n>;  n sprite records  x,y,cell,visible  then n touch rectangles  x,y,half w,half h,enabled
Sprite slots are numbered in file order across the groups (slot k = the k-th sprite record)."""
import os
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
LAYOUTS = {'calendar': 1, 'profile': 2, 'sound': 3, 'display': 4}


def _ints(line):
    return [int(x) for x in line.strip().rstrip(';').split(',') if x.strip() != '']


def parse(text):
    lines = [l.strip() for l in text.replace('\r', '').split('\n')]
    lines = [l for l in lines if l]
    out = dict(arc=None, bg={}, pal={}, obj={}, slots=[])
    i = 0
    while i < len(lines):
        l = lines[i]
        if l.startswith('ARC_FILE_PATH:'):
            out['arc'] = l.split(':', 1)[1].rstrip(';'); i += 1
        elif l.endswith('_RESOURCE_ARC:') and l.startswith('BG_'):
            eng = 'main' if 'MAIN' in l else 'sub'
            n = _ints(lines[i + 1])[0]
            out['bg'][eng] = [(e.split(':')[0], _ints(e.split(':')[1])) for e in lines[i + 2:i + 2 + n]]
            i += 2 + n
        elif l.endswith('_COLOR_PAL_ARC:'):
            key = l[:-len('_COLOR_PAL_ARC:')].lower()
            n = _ints(lines[i + 1])[0]
            out['pal'][key] = [tuple(_ints(e)) for e in lines[i + 2:i + 2 + n]]
            i += 2 + n
        elif l.startswith('OBJ_') and l.endswith('_RESOURCE_ARC:'):
            eng = 'main' if 'MAIN' in l else 'sub'
            ngroups = _ints(lines[i + 1])[0]
            i += 3                                           # groups; total sprites
            groups = []
            for _ in range(ngroups):
                head = lines[i]
                res = head.split(':', 1)[1].rstrip(';')
                res = tuple(_ints(res)) if res[:1].isdigit() else res
                n = _ints(lines[i + 1])[0]
                recs = [tuple(_ints(x)) for x in lines[i + 2:i + 2 + n]]
                hits = [tuple(_ints(x)) for x in lines[i + 2 + n:i + 2 + 2 * n]]
                # padding records (0,0,0,0,0) follow some groups: skip them
                j = i + 2 + 2 * n
                while j < len(lines) and lines[j].rstrip(';') == '0,0,0,0,0':
                    j += 1
                groups.append(dict(resource=res, sprites=recs, hits=hits))
                for k, r in enumerate(recs):
                    h = hits[k] if k < len(hits) else None
                    out['slots'].append(dict(slot=len(out['slots']), engine=eng, resource=res, x=r[0], y=r[1], cell=r[2],
                                             visible=bool(r[3]),
                                             hit=(h[0], h[1], h[2], h[3]) if h and len(h) >= 5 and h[4] else None))
                i = j
            out['obj'][eng] = groups
        else:
            i += 1
    return out


def load(name):
    p = os.path.join(ROOT, 'content/assets/menu/mount_layout.bin', '%04d.bin' % LAYOUTS[name])
    return parse(open(p, 'rb').read().decode('ascii', 'replace'))
