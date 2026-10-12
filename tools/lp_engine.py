"""Accessor for the extracted engine data (content/engine). Read bytes/values by module + RAM address, look up cataloged tables.

    eng = Engine('content')
    eng.u32('static', 0x21067f0)            # typed reads
    eng.table('event_id_table_kind3')        # -> bytes of a named table
    eng.cstr('static', 0x212427d)            # NUL-terminated UTF-8 string
    eng.find_table(0x2113c10)                # catalog entry covering an address
Overlays that share a base address are separate modules ('ov05', 'ov41', ...); pass the one you want.
"""
import json, os, struct, bisect


class Engine:
    def __init__(self, root):
        d = os.path.join(root, 'engine')
        self.mm = json.load(open(os.path.join(d, 'memory_map.json')))
        self.img = {k: open(os.path.join(d, k + '.bin'), 'rb').read() for k in self.mm}
        self.cat = json.load(open(os.path.join(d, 'tables.json')))['tables']
        self.names = {t['name']: t for t in self.cat if 'name' in t}
        self._idx = {}
        for t in self.cat:
            self._idx.setdefault(t['module'], []).append((int(t['addr'], 16), t))
        for v in self._idx.values(): v.sort(key=lambda x: x[0])

    def read(self, module, addr, n):
        base = self.mm[module]['base']; a = addr - base
        if a < 0 or a + n > len(self.img[module]): raise IndexError('%s:%#x+%d outside image' % (module, addr, n))
        return self.img[module][a:a + n]

    def u8(self, m, a): return self.read(m, a, 1)[0]
    def u16(self, m, a): return struct.unpack('<H', self.read(m, a, 2))[0]
    def s16(self, m, a): return struct.unpack('<h', self.read(m, a, 2))[0]
    def u32(self, m, a): return struct.unpack('<I', self.read(m, a, 4))[0]
    def s32(self, m, a): return struct.unpack('<i', self.read(m, a, 4))[0]

    def cstr(self, m, a, enc='utf-8'):
        base = self.mm[m]['base']; i = a - base; j = self.img[m].index(b'\0', i)
        return self.img[m][i:j].decode(enc, 'replace')

    def table(self, name):
        t = self.names[name]
        return self.read(t['module'], int(t['addr'], 16), t.get('size', t['size_est']))

    def find_table(self, addr, module='static'):
        lst = self._idx.get(module, []); i = bisect.bisect_right([a for a, _ in lst], addr) - 1
        if i >= 0:
            a, t = lst[i]
            if addr < a + t['size_est']: return t
        return None
