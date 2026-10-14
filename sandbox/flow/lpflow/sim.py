"""Running the ported flow machines without the game: SimCtx executes them against a World.

SimCtx implements what belongs to the flow itself natively (the state stack Main_*, the screen-side request accessors)
and keeps memory as a byte dict; every other game function is answered by the World's handler for its address, or
returns 0 and is counted in `unhandled`. A World is a set of models of the subsystems the flow drives: screens (what
each screen does frame by frame), fades, flags, the scene overlay's polls ... scripted or simulated."""
from .ctx import Out, s32
from .names import F, W, NAME

M = 0xffffffff
STACK_DEPTH, STACK_STATES = 0x2139cdc, 0x2139ce0
SB = W(0x20885f0)                  # 0x214fde0: the running screen's request block (+4 next screen, +0xc mode,
                                   #   +0x14 result, +0x1c alternative result, +0x24 switch, +0x28 finish)


class SimCtx:
    def __init__(self, ports, world):
        self.ports = ports
        self.world = world
        self.mem = {}
        self.unhandled = {}
        self.where = []
        self.frame = 0
        self.trace = []                # (frame, depth, machine, old state, new state)
        self.native = {
            F.Main_IncStateDepth: self._inc, F.Main_DecStateDepth: self._dec,
            F.Main_GetStateAtDepth: self._get, F.Main_SetState: self._set,
            F.Main_AdvanceStateSequence: lambda: self._set(self._get() + 1),
            F.Screen_GetNext: lambda: self.load(SB + 4, signed=True),
            F.Screen_GetResult: lambda: self.load(SB + 0x14, signed=True),
            F.Screen_GetAltResult: lambda: self.load(SB + 0x1c, signed=True),
            0x20885f4: lambda: self.load(SB + 0x10, signed=True),
            0x208a1f0: lambda: self.load(W(0x208a1fc) + 0x1c, signed=True),        # the entry argument
            0x208a200: lambda v: self.store(W(0x208a20c) + 0x1c, v),
        }
        self.stores = 0

    # ---- memory -------------------------------------------------------------------------------------------------
    def load(self, addr, size=4, signed=False):
        v = 0
        for i in range(size):
            v |= self.mem.get((addr + i) & M, 0) << (8 * i)
        if signed and v >> (8 * size - 1):
            v -= 1 << (8 * size)
        return v

    def store(self, addr, value, size=4):
        if value is None:
            return
        for i in range(size):
            self.mem[(addr + i) & M] = (value >> (8 * i)) & 0xff

    def out(self, n):
        o = Out(n)
        for i in range(n):
            o[i] = 0
        return o

    def pick(self, cond, targets):
        return cond

    # ---- the state stack ----------------------------------------------------------------------------------------
    def _depth(self):
        return self.load(STACK_DEPTH)

    def _inc(self):
        self.store(STACK_DEPTH, self._depth() + 1)

    def _dec(self):
        self.store(STACK_DEPTH, self._depth() - 1)

    def _get(self):
        return self.load(STACK_STATES + 4 * self._depth(), signed=True)

    def _set(self, s):
        d = self._depth()
        old = self._get()
        self.store(STACK_STATES + 4 * d, s)
        for i in range(d + 1, 15):
            self.store(STACK_STATES + 4 * i, 0)
        if old != s:
            self.trace.append((self.frame, d, self.where[-1] if self.where else '-', old, s))

    # ---- calls --------------------------------------------------------------------------------------------------
    def call(self, target, *args):
        if target in self.ports:
            name, fn = self.ports[target]
            self.where.append(name)
            r = fn(self, *args)
            self.where.pop()
            return s32(r) if r is not None else 0
        if target in self.native:
            r = self.native[target](*args)
            return s32(r) if r is not None else 0
        h = self.world.handler(target)
        if h is not None:
            r = h(self, *args)
            return s32(r) if r is not None else 0
        name = NAME.get(target, '%x' % target)
        self.unhandled[name] = self.unhandled.get(name, 0) + 1
        return 0

    def step(self):
        """one frame of the game's flow (GameHeap_UpdateGameState -> Main_ScopedFadeCheck)"""
        self.world.frame(self)
        r = self.call(0x203fb20)
        self.frame += 1
        return r


class World:
    """handlers by game address; subclass and register"""
    def __init__(self):
        self.handlers = {}

    def on(self, target, fn):
        self.handlers[target] = fn

    def handler(self, target):
        return self.handlers.get(target)

    def frame(self, cx):
        pass
