"""Address-interval map (buffer -> origin), later writes win; O(log n) lookups for the replay's online provenance."""
import bisect


class BufMap:
    def __init__(self):
        self.starts = []          # sorted interval starts
        self.items = {}           # start -> (end, origin)

    def put(self, start, size, origin):
        if size <= 0:
            return
        end = start + size
        i = bisect.bisect_right(self.starts, start) - 1
        if i >= 0:                                   # an interval starting before `start` that overlaps: trim it
            s = self.starts[i]
            e, o = self.items[s]
            if e > start:
                self.items[s] = (start, o)
                if e > end:                          # it also extends past the new one: keep its tail
                    self._insert(end, e, dict(o, off=o.get('off', 0) + (end - s)) if o else None)
        j = bisect.bisect_left(self.starts, start)
        while j < len(self.starts) and self.starts[j] < end:
            s = self.starts[j]
            e, o = self.items.pop(s)
            del self.starts[j]
            if e > end:
                self._insert(end, e, dict(o, off=o.get('off', 0) + (end - s)) if o else None)
                break
        self._insert(start, end, origin)

    def _insert(self, s, e, o):
        if s in self.items:
            self.items[s] = (e, o); return
        bisect.insort(self.starts, s)
        self.items[s] = (e, o)

    def find(self, a):
        """(start, end, origin) of the interval containing a"""
        i = bisect.bisect_right(self.starts, a) - 1
        if i < 0:
            return None
        s = self.starts[i]
        e, o = self.items[s]
        return (s, e, o) if a < e and o is not None else None

    def origin(self, a):
        hit = self.find(a)
        if not hit:
            return None
        s, e, o = hit
        return dict(o, off=o.get('off', 0) + (a - s))
