"""Execution contexts for the ported flow machines.

A ported machine is plain Python that does all its I/O through a context:
    cx.call(target, *args)    call a game function (by address); returns its r0 as a signed 32-bit value. A target
                              that is itself ported (cx.ports) runs the Python port instead.
    cx.load(addr, size=4, signed=False) / cx.store(addr, value, size=4)   a global (non-stack) memory access
    cx.out(n)                 an n-word stack buffer a callee fills (GameClock_GetDate(&buf) ...)
    cx.pick(cond, {value: target, ...})   a branch on data the replay cannot see (stack buffers): the replay
                              resolves it from the next call target, a live context evaluates `cond`

LogCtx replays a log recorded from the game (sandbox/flow/calllog.py) and checks the port against it: call targets
in order, the arguments the port gives, the loads and stores of each stretch between two calls (as a multiset: the
compiler's ordering is free), and the return value of every ported function."""
M = 0xffffffff


def s32(v):
    v &= M
    return v - (1 << 32) if v >> 31 else v


class Mismatch(Exception):
    pass


class Out(list):
    """a stack buffer a callee fills: pass it as the argument; read its words after the call (None until filled)"""
    def __init__(self, n):
        super().__init__([None] * n)


class LogCtx:
    def __init__(self, path, ports):
        self.ports = ports
        self.recs = []
        self.start_stack = None
        for line in open(path):
            p = line.split()
            if not p:
                continue
            k = p[0]
            if k == 'S':
                self.start_stack = [int(x, 16) for x in p[1:]]
            elif k == 'f':
                self.recs.append(('f', int(p[1], 16)))
            elif k == 'c':
                self.recs.append(('c',) + tuple(int(x, 16) for x in p[1:]))
            elif k == 'r':
                self.recs.append(('r', int(p[1], 16), int(p[2], 16)))
            elif k == 'F':
                self.recs.append(('F', int(p[1], 16), int(p[2], 16), int(p[3], 16)))
            elif k == 'o':
                self.recs.append(('o', int(p[1], 16), int(p[2])) + tuple(int(x, 16) for x in p[3:]))
            elif k in 'ls':
                a = int(p[1], 16)
                if 0x27e0000 <= a < 0x27e4000:         # the stack (DTCM) reached through another register: locals
                    continue
                self.recs.append((k, a, int(p[2]), int(p[3], 16)))
        self.i = 0
        self.used = set()
        self.frame = None
        self.warnings = []
        self.counts = dict(calls=0, ported=0, loads=0, stores=0)
        self.where = []                       # stack of ported functions being run (for messages)
        self.coverage = {}                    # (machine, state) -> frames seen

    # ---- helpers ------------------------------------------------------------------------------------------------
    def _ctx(self):
        return 'frame %s, in %s' % (self.frame, ' > '.join(self.where) or '-')

    def _segment(self):
        j = self.i
        while j < len(self.recs) and self.recs[j][0] in 'ls':
            yield j
            j += 1

    def _end_segment(self):
        j = self.i
        while j < len(self.recs) and self.recs[j][0] in 'ls':
            if j not in self.used:
                k, a, n, v = self.recs[j]
                self.warnings.append('%s: game %s %x (%d bytes, %x) the port did not make'
                                     % (self._ctx(), 'loaded' if k == 'l' else 'STORED', a, n, v))
            j += 1
        self.i = j

    def _skip_call(self, site):
        """past the return of the call made at `site` (whatever the callee did is not ours)"""
        depth = 0
        j = self.i
        while j < len(self.recs):
            r = self.recs[j]
            if r[0] == 'c' and r[1] == site:
                depth += 1
            elif r[0] == 'o':
                pass
            elif r[0] == 'r' and r[1] == site:
                if depth == 0:
                    if j > 0 and self.recs[j - 1][0] == 'F':
                        self.counts['forced'] = self.counts.get('forced', 0) + 1
                    self.i = j + 1
                    return r[2]
                depth -= 1
            j += 1
        raise EOFError('log ended inside a call from %x' % site)

    # ---- the interface ------------------------------------------------------------------------------------------
    def load(self, addr, size=4, signed=False):
        addr &= M
        for j in self._segment():
            r = self.recs[j]
            if j not in self.used and r[0] == 'l' and r[1] == addr and r[2] == size:
                self.used.add(j)
                self.counts['loads'] += 1
                v = r[3]
                if signed:
                    b = 8 * size
                    v = v - (1 << b) if v >> (b - 1) else v
                return v
        raise Mismatch('%s: port loads %x (%d bytes); the game did not' % (self._ctx(), addr, size))

    def store(self, addr, value, size=4):
        """value None: a value the replay cannot know (from a stack buffer); only the address is checked"""
        addr &= M
        if value is not None:
            value &= (1 << (8 * size)) - 1
        for j in self._segment():
            r = self.recs[j]
            if j not in self.used and r[0] == 's' and r[1] == addr and r[2] == size:
                self.used.add(j)
                self.counts['stores'] += 1
                if value is not None and r[3] != value:
                    raise Mismatch('%s: port stores %x to %x; the game stored %x' % (self._ctx(), value, addr, r[3]))
                return
        raise Mismatch('%s: port stores %x to %x (%d bytes); the game did not' % (self._ctx(), value, addr, size))

    def out(self, n):
        return Out(n)

    def _fill(self, site, args):
        """the 'o' records after a return: fill the Out arguments"""
        while self.i < len(self.recs) and self.recs[self.i][0] == 'o' and self.recs[self.i][1] == site:
            r = self.recs[self.i]
            k = r[2]
            if k < len(args) and isinstance(args[k], Out):
                for j in range(min(len(args[k]), 4)):
                    args[k][j] = r[3 + j]
            self.i += 1

    def pick(self, cond, targets):
        self._end_segment()
        r = self.recs[self.i]
        for value, t in targets.items():
            if r[0] == 'c' and r[2] == t:
                return value
        raise Mismatch('%s: next call %s matches none of %s' % (self._ctx(), r, targets))

    def call(self, target, *args):
        self._end_segment()
        if self.i >= len(self.recs):
            raise EOFError
        r = self.recs[self.i]
        if r[0] != 'c':
            raise Mismatch('%s: port calls %x; the game: %s' % (self._ctx(), target, r))
        _, site, tgt = r[:3]
        if tgt != target:
            raise Mismatch('%s: port calls %x; the game called %x (site %x)' % (self._ctx(), target, tgt, site))
        for k, a in enumerate(args[:4]):
            if isinstance(a, int) and (a & M) != r[3 + k]:
                raise Mismatch('%s: call %x (site %x) argument %d: port %x, game %x'
                               % (self._ctx(), target, site, k, a & M, r[3 + k]))
        self.i += 1
        self.counts['calls'] += 1
        if target in self.ports:
            name, fn = self.ports[target]
            self.where.append(name)
            res = fn(self, *args)
            self.where.pop()
            self._end_segment()
            forced = None
            if self.recs[self.i][0] == 'F' and self.recs[self.i][1] == site:
                forced = self.recs[self.i]
                self.i += 1
            rr = self.recs[self.i]
            if rr[0] != 'r' or rr[1] != site:
                raise Mismatch('%s: %s returned; the game still had %s' % (self._ctx(), name, rr))
            self.i += 1
            self._fill(site, args)
            self.counts['ported'] += 1
            game = forced[2] if forced else rr[2]
            if res is not None and (res & M) != game:
                raise Mismatch('%s: %s returned %x; the game %x' % (self._ctx(), name, res & M, game))
            if forced:
                self.counts['forced'] = self.counts.get('forced', 0) + 1
            return s32(rr[2])
        v = self._skip_call(site)
        self._fill(site, args)
        if target == 0x203dbd4 and self.where:                 # Main_GetStateAtDepth: which state ran
            k = (self.where[-1], s32(v))
            self.coverage[k] = self.coverage.get(k, 0) + 1
        return s32(v)

    # ---- driving ------------------------------------------------------------------------------------------------
    def frames(self):
        """yield at every frame boundary of the log (a log that starts inside a frame - a save state taken mid-frame -
        skips to the first boundary)"""
        while self.i < len(self.recs) and self.recs[self.i][0] != 'f':
            self.i += 1
        while self.i < len(self.recs):
            r = self.recs[self.i]
            if r[0] != 'f':
                raise Mismatch('frame %s: records left over after the frame: %s' % (self.frame, r))
            self.frame = r[1]
            self.i += 1
            yield self.frame
