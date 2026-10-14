"""Probe: the sprite objects of a capture with their resource files (handle -> resource table data pointer -> traced load),
sequence, position and current cell index."""
import json, os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, os.path.dirname(HERE))
from lpui import sprites

B = 0x2000000


def loaded_buffers(trace_dir):
    """[(frame, ptr, arc, entry)] of resource loads (res_loaded / res_read)"""
    out = []
    for l in open(os.path.join(trace_dir, 'trace.jsonl')):
        if '"res_loaded"' in l or '"res_read"' in l:
            r = json.loads(l)
            out.append((r['f'], r.get('ptr', r.get('dest')), r['arc'], r['entry']))
    return out


def resource_file(ram, handle, bufs, frame):
    e = (handle & 0x3ff) * 0x18 + 0x213e5f4 - B
    data = struct.unpack_from('<I', ram, e)[0]
    best = None
    for f, ptr, arc, entry in bufs:
        if f <= frame and ptr is not None and ptr <= data < ptr + 0x40000 and (best is None or ptr >= best[0]):
            best = (ptr, arc, entry)
    return (best[1], best[2], data - best[0]) if best else (None, None, data)


def objects(capture, bufs, frame):
    ram = open(os.path.join(capture, 'ram.bin'), 'rb').read()
    out = []
    for li in range(0x3f, -1, -1):
        p = struct.unpack_from('<I', ram, 0x213f6a0 - B + 4 * li)[0]
        while B <= p < B + 0x400000:
            o = sprites.SprObj(ram, p)
            if o.kind:
                cb = resource_file(ram, o.cell_bank, bufs, frame)
                out.append(dict(obj=hex(p), eng=o.engine, kind=o.kind, x=o.x >> 16, y=o.y >> 16, seq=o.seq, cells=cb[:2],
                                anim=resource_file(ram, o.anim_bank, bufs, frame)[:2], prio=o.prio, lst=o.lst))
            p = o.next
    return out
