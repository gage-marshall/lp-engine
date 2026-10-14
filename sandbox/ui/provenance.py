"""Where each piece of VRAM came from, from a replay trace (sandbox/ui/replay_trace.py): VRAM load source buffers followed
back through LZ8 decompression to (archive, entry, offset in the file). Per capture: the last load into every VRAM target.
    .venv/bin/python sandbox/ui/provenance.py TRACE_DIR [--json OUT]"""
import bisect, json, os, sys
from collections import defaultdict


class Buffers:
    """address intervals -> origin; a later write over the same bytes wins"""
    def __init__(self):
        self.iv = []        # (start, end, origin)

    def put(self, start, size, origin):
        end = start + size
        self.iv = [x for x in self.iv if x[1] <= start or x[0] >= end] + [(start, end, origin)]

    def find(self, a):
        best = None
        for s, e, o in self.iv:
            if s <= a < e and (best is None or s > best[0]):
                best = (s, e, o)
        return best


_SIZES = {}


def file_size(arc, entry):
    """size of an archive entry as extracted (content/assets/<arc>/<entry>.*)"""
    if arc is None:
        return None
    key = (arc, entry)
    if key not in _SIZES:
        import glob
        root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        fs = glob.glob(os.path.join(root, 'content/assets', arc, '%04d.*' % entry))
        _SIZES[key] = os.path.getsize(fs[0]) if fs else None
    return _SIZES[key]


def origin_of(buf, src):
    hit = buf.find(src)
    if not hit:
        return None
    s, e, o = hit
    return dict(o, off=o.get('off', 0) + (src - s))


def analyse(trace_dir):
    tables = {}
    buf = Buffers()
    last = {}              # VRAM target key -> origin record
    caps = {}
    for line in open(os.path.join(trace_dir, 'trace.jsonl')):
        r = json.loads(line)
        ev = r['ev']
        if ev == 'res_open':
            tables[r['slot']] = r['name']
        elif ev == 'res_read':
            buf.put(r['dest'], r['size'], dict(arc=tables.get(r['slot']), entry=r['entry']))
        elif ev == 'res_loaded':
            name = tables.get(r['slot'])
            size = file_size(name, r['entry']) or 0x10000
            buf.put(r['ptr'], size, dict(arc=name, entry=r['entry']))
        elif ev == 'copy':
            o = buf.find(r['src'])
            if o and r['size'] > 0:
                buf.put(r['dst'], r['size'], dict(o[2], off=o[2].get('off', 0) + (r['src'] - o[0])))
        elif ev == 'lz8':
            o = buf.find(r['src'])
            if o:
                buf.put(r['dst'], r['size'], dict(o[2], lz=True, off=0))
            else:
                buf.put(r['dst'], r['size'], dict(arc=None, gen='lz-unknown'))
        elif ev == 'gx':
            if r['what'].endswith('oam'):
                continue
            o = origin_of(buf, r['src'])
            last[(r['what'], r['off'])] = dict(size=r['size'], src=r['src'], origin=o, f=r['f'], lr=r['lr'])
        elif ev == 'vram_xfer':
            o = origin_of(buf, r['src'])
            last[('xfer%d' % r['type'], r['dst'])] = dict(size=r['size'], src=r['src'], origin=o, f=r['f'], lr=r['lr'])
        elif ev == 'capture':
            caps[r['path']] = {'%s@%#x' % k: v for k, v in last.items()}
    return caps


def summary(entry):
    o = entry['origin']
    if not o:
        return 'GENERATED (lr %#x)' % entry['lr']
    return '%s/%04d%s +%#x' % (o.get('arc'), o.get('entry', -1), ' lz' if o.get('lz') else '', o.get('off', 0))


if __name__ == '__main__':
    caps = analyse(sys.argv[1])
    if '--json' in sys.argv:
        json.dump(caps, open(sys.argv[sys.argv.index('--json') + 1], 'w'), indent=0)
    names = sorted(caps, key=lambda p: int(p.rsplit('-f', 1)[1]))
    for p in names[-1:]:
        print(p)
        for k, v in sorted(caps[p].items()):
            print('  %-24s %6d bytes  %s' % (k, v['size'], summary(v)))
