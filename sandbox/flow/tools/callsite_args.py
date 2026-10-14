"""For every call site (BL / BLX) in the ported flow functions: which argument registers r0..r3 the game sets up for
the call, from its machine code. A register counts when it is written after the previous call (or branch target) and
before this one; r0 also counts when nothing wrote it since the previous call (the previous call's result passed
through). Output: JSON {site: [target, highest argument count, pass-through]}.
    .venv/bin/python sandbox/flow/tools/callsite_args.py > sandbox/flow/logs/callsite_args.json"""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
from calllog import PORTED, sizes  # noqa
from lpflow.names import G  # noqa
from capstone import Cs, CS_ARCH_ARM, CS_MODE_ARM  # noqa
md = Cs(CS_ARCH_ARM, CS_MODE_ARM)
md.detail = True
md.skipdata = True
base, img = G._image(None)
sz = sizes()

out = {}
for start, name in PORTED.items():
    ins = list(md.disasm(bytes(img[start - base:start - base + sz[start]]), start))
    # branch targets inside the function start new blocks (a register written before a join may be dead on the other path)
    targets = set()
    for i in ins:
        if i.mnemonic.startswith('b') and i.op_str.startswith('#'):
            try:
                targets.add(int(i.op_str[1:], 16))
            except ValueError:
                pass
    written = set()
    since_call = True
    for i in ins:
        if i.address in targets:
            written = set()
        if i.mnemonic in ('bl', 'blx'):
            regs = [r for r in range(4) if r in written]
            n = max(regs) + 1 if regs else 0
            passthrough = 0 not in written
            tgt = i.op_str[1:] if i.op_str.startswith('#') else i.op_str
            out['%x' % i.address] = [tgt, n, passthrough]
            written = set()
            continue
        # registers this instruction writes
        try:
            _, w = i.regs_access()
            for r in w:
                nm = i.reg_name(r)
                if nm in ('r0', 'r1', 'r2', 'r3'):
                    written.add(int(nm[1]))
        except Exception:
            pass
json.dump(out, sys.stdout, indent=0)
