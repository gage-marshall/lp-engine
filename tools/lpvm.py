"""Reference LovePlus+ script VM (portable engine-core seed).

A Script wraps a decrypted SBIN. run(entry) is a generator that yields host requests:
  ('cmd', id, args)            host executes game command id; may reply via send(value) -> acc
  ('text', {'name':[..],'text':[..],'sub':[..]}, mode)   text box commit (op 0x83 mode=None, 0x84 mode=arg)
  ('wait_fade',)               op 0x88
  ('end', acc)                 entry returned
Segments in text buffers are ('s', str) literals or ('v', id) string-variable references.
Semantics verified against Script_ExecuteOpcode @0x204cb48 (see docs/SCRIPT_VM.md).
"""
import os, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
from lp_decrypt import decrypt

M32 = 0xffffffff


def s16(v):
    return v - 65536 if v & 0x8000 else v


def read_typed_cell(memory, descriptor):
    """Read the VM's ``type:size-or-bit:offset`` variable descriptor."""
    kind = (descriptor >> 28) & 0xf
    width_or_bit = (descriptor >> 24) & 0xf
    offset = descriptor & 0xffffff
    if kind == 4 and width_or_bit in (1, 2, 4) and offset + width_or_bit <= len(memory):
        return int.from_bytes(memory[offset:offset + width_or_bit], 'little')
    if kind == 8 and offset < len(memory):
        return int(bool(memory[offset] & (1 << width_or_bit)))
    return 0


def write_typed_cell(memory, descriptor, value):
    """Write a typed VM cell; immutable/unknown descriptor types are ignored."""
    kind = (descriptor >> 28) & 0xf
    width_or_bit = (descriptor >> 24) & 0xf
    offset = descriptor & 0xffffff
    if kind == 4 and width_or_bit in (1, 2, 4):
        required = offset + width_or_bit
        if required > len(memory):
            memory.extend(b'\0' * (required - len(memory)))
        memory[offset:required] = (value & ((1 << (width_or_bit * 8)) - 1)).to_bytes(width_or_bit, 'little')
    elif kind == 8:
        if offset >= len(memory):
            memory.extend(b'\0' * (offset + 1 - len(memory)))
        mask = 1 << width_or_bit
        memory[offset] = (memory[offset] | mask) if value else (memory[offset] & ~mask)


class Script:
    def __init__(self, raw=None, dec=None):
        self.d = dec if dec is not None else decrypt(raw)
        h = struct.unpack('>11I', self.d[8:0x34])
        self.hdr = h
        n = h[0]
        self.entries = list(struct.unpack('>%dI' % n, self.d[0x40:0x40 + 4 * n])) if n else []
        self.str_base = h[1]
        self.ids = {}
        cnt, off = h[10], h[9]
        for k in range(cnt):
            if off and off + 8 * k + 8 <= len(self.d):
                i, ref = struct.unpack('>II', self.d[off + 8 * k:off + 8 * k + 8])
                self.ids[i] = ref
        # Header fields +0x20/+0x28 describe local typed-cell declarations.
        # Most LovePlus+ scripts have none, which intentionally makes local
        # reads zero and writes no-ops in the native VM.
        local_off, local_count = h[6], h[8]
        if local_count and local_off + 4 * local_count <= len(self.d):
            self.local_descriptors = list(struct.unpack('>%dI' % local_count,
                                                        self.d[local_off:local_off + 4 * local_count]))
        else:
            self.local_descriptors = []

    def string(self, off):
        p = self.str_base + (off & 0x7fffffff)
        n = self.d[p]; q = p + 1
        if n & 0x80:
            n = self.d[p + 1] + 0x80; q = p + 2
        return self.d[q:q + n].decode('utf-8', 'replace')

    def run(self, entry, max_steps=200000):
        d = self.d
        pc = self.entries[entry]
        acc = 0; flag = 0
        stack = []
        frames = []
        regs = {}; gvars = {}; local_memory = bytearray(0x400)
        bufs = [[], [], []]
        steps = 0
        while True:
            steps += 1
            if steps > max_steps:
                yield ('limit', pc); return
            op, n = d[pc], d[pc + 1]
            a = struct.unpack('>%dH' % n, d[pc + 2:pc + 2 + 2 * n])
            nxt = pc + 2 + 2 * n
            if op == 0x00: pass
            elif op == 0x01: stack.append(acc)
            elif op == 0x02: acc = stack.pop() if stack else 0
            elif op == 0x03:
                for _ in range(a[0]):
                    if stack: stack.pop()
            elif op == 0x10: nxt = nxt + s16(a[0])
            elif op == 0x11:
                if flag: nxt = nxt + s16(a[0])
            elif op == 0x12:
                if not flag: nxt = nxt + s16(a[0])
            elif op in (0x13, 0x14):
                if op == 0x14:
                    acc = self.ids.get((a[0] << 16) | a[1], 0)
                if not frames:
                    yield ('end', acc); return
                nxt = frames.pop()
            elif op == 0x16:
                ref = self.ids.get((a[0] << 16) | a[1], 0)
                if ref:
                    tgt = self.entries[ref & 0xfff] if (ref & 0xfff) < len(self.entries) else None
                    if tgt is not None:
                        frames.append(nxt); nxt = tgt
            elif op == 0x20: flag = int(acc != 0)
            elif op == 0x21: flag = int(bool(stack) and acc == stack[-1])
            elif op == 0x30: acc = 0
            elif op == 0x31: acc = 1
            elif op == 0x32: acc = a[0]
            elif op == 0x33: acc = (a[0] << 16) | a[1]
            elif op == 0x34: acc = int(acc != 0)
            elif op == 0x35: acc = (-acc) & M32
            elif op == 0x40: acc = regs.get(a[0], 0)
            elif op == 0x41: acc = gvars.get(a[0], 0)
            elif op == 0x42:
                acc = read_typed_cell(local_memory, self.local_descriptors[a[0]]) if a[0] < len(self.local_descriptors) else 0
            elif op == 0x43: regs[a[0]] = acc
            elif op == 0x44: gvars[a[0]] = acc
            elif op == 0x45:
                if a[0] < len(self.local_descriptors): write_typed_cell(local_memory, self.local_descriptors[a[0]], acc)
            elif 0x50 <= op <= 0x5c:
                b = stack.pop() if stack else 0
                x = stack.pop() if stack else 0
                if op == 0x50: acc = (x + b) & M32
                elif op == 0x51: acc = (x - b) & M32
                elif op == 0x52: acc = (x * b) & M32
                elif op == 0x53: acc = 0 if b == 0 else (int(abs(s32(x)) // abs(s32(b))) * (1 if (s32(x) < 0) == (s32(b) < 0) else -1)) & M32
                elif op == 0x54: acc = x & b
                elif op == 0x55: acc = x | b
                elif op == 0x56: acc = x ^ b
                elif op == 0x57: acc = int(x == b)
                elif op == 0x58: acc = int(x != b)
                elif op == 0x59: acc = int(x < b)
                elif op == 0x5a: acc = int(x <= b)
                elif op == 0x5b: acc = int(x > b)
                elif op == 0x5c: acc = int(x >= b)
            elif op == 0x80: bufs[a[0]] = []
            elif op == 0x81: bufs[a[0]].append(('s', self.string((a[1] << 16) | a[2])))
            elif op == 0x82: bufs[a[0]].append(('v', a[1]))
            elif op == 0x83:
                yield ('text', {'text': list(bufs[0]), 'sub': list(bufs[1]), 'name': list(bufs[2])}, None)
            elif op == 0x84:
                yield ('text', {'text': list(bufs[0]), 'sub': list(bufs[1]), 'name': list(bufs[2])}, a[0])
            elif op in (0x85, 0x86): pass
            elif op == 0x87:
                k = stack[-1] if stack else 0
                args = stack[-1 - k:-1] if k else []
                r = yield ('cmd', (a[0] << 16) | a[1], list(args))
                if r is not None:
                    acc = r & M32
            elif op == 0x88: yield ('wait_fade',)
            elif 0x5d <= op <= 0x7f or op in (0x04,): pass
            else:
                yield ('badop', op, pc); return
            pc = nxt


def s32(v):
    return v - (1 << 32) if v & 0x80000000 else v


if __name__ == '__main__':
    import random, json, collections
    src = 'work/assets/arc/tmx_event.arc'
    acc_cmds = {int(k, 16) for k in json.load(open('fid/cmd_acc_writes.json'))}
    res = collections.Counter(); badops = collections.Counter(); steps_total = 0
    cmdcount = collections.Counter(); texts = 0
    for f in sorted(os.listdir(src)):
        sc = Script(open(os.path.join(src, f), 'rb').read())
        for e in range(len(sc.entries)):
            for seed in (0, 1):
                rnd = random.Random(seed * 1000 + e)
                g = sc.run(e, max_steps=20000)
                reply = None
                try:
                    while True:
                        ev = g.send(reply); reply = None
                        if ev[0] == 'cmd':
                            cmdcount[ev[1]] += 1
                            if ev[1] in acc_cmds: reply = rnd.choice((0, 1)) if seed else 0
                        elif ev[0] == 'text': texts += 1
                        elif ev[0] in ('end', 'limit', 'badop'):
                            res[(seed, ev[0])] += 1
                            if ev[0] == 'badop': badops[ev[1]] += 1
                            break
                except (IndexError, struct.error) as ex:
                    res[(seed, 'fault')] += 1
    print(dict(res)); print('badops', dict(badops)); print('text commits', texts, 'distinct commands used', len(cmdcount))
