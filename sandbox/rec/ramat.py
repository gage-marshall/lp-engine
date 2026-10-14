"""Main RAM at any frame of a record.py recording.

    from ramat import Recording
    rec = Recording('sandbox/rec/out/s08143402'); ram = rec.at(6381)   # absolute monitor frame -> np.uint32[0x100000]
Frame convention: rec.at(F) = RAM after the emulator has run F frames (same numbering as play_monitor 'f' and gxcap).
"""
import json, os
import numpy as np


class Recording:
    def __init__(self, path):
        self.meta = json.load(open(os.path.join(path, 'meta.json')))
        self.f0 = self.meta['anchor_frame']
        self.ram0 = np.fromfile(os.path.join(path, 'ram0.bin'), '<u4')
        z = np.load(os.path.join(path, 'diff.npz'))
        self.count, self.idx, self.val = z['count'], z['idx'], z['val']
        self.ofs = np.concatenate([[0], np.cumsum(self.count.astype(np.int64))])
        self._cache = (self.f0, self.ram0.copy())

    def at(self, frame):
        k = frame - self.f0          # number of replayed frames applied
        assert 0 <= k <= len(self.count), 'frame outside recording'
        cf, ram = self._cache
        if cf > frame:
            cf, ram = self.f0, self.ram0.copy()
        for j in range(cf - self.f0, k):
            a, b = self.ofs[j], self.ofs[j + 1]
            ram[self.idx[a:b]] = self.val[a:b]
        self._cache = (frame, ram)
        return ram.copy()

    def series(self, addr, frames):
        """value of the u32 at addr for each frame"""
        i = (addr - 0x2000000) // 4
        out = []
        for f in frames:
            out.append(int(self.at(f)[i]))
        return out
