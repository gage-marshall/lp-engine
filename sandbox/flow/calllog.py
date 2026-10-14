"""Record every call the game's flow machines make, for checking the Python port (lpflow) call by call.

The machines (PORTED) are disassembled from the game image; every BL / BLX site inside them gets an exec hook (the call:
site, target, r0..r3) and one on the instruction after it (the return: r0, only while a call from that site is open).
Output, one record per line (hex):
    c SITE TARGET R0 R1 R2 R3        a call
    r SITE R0                        its return
    o SITE ARG W0 W1 W2 W3           after a return: the 4 words behind argument ARG when it points into the stack
                                     (DTCM 0x27e0000..0x27e4000): what the callee wrote into a caller's buffer
    l ADDR SIZE VALUE                a load the machine code made (not stack, not literal pool)
    s ADDR SIZE VALUE                a store
    F SITE ORIGINAL FORCED           the explorer replaced the return value of the call at SITE (explore_calls.py)
    f FRAME                          a new frame
    S DEPTH STATE0 .. STATE14        the state stack when the log starts
OUT.txt.ram holds main RAM at the first frame record.
    .venv/bin/python sandbox/flow/calllog.py SESSION.jsonl OUT.txt [LAST_FRAME]"""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
sys.path.insert(0, os.path.join(ROOT, 'sandbox/ui'))
from lpui import gamedata as G  # noqa: E402

# the ported machines (static code): start address -> name; sizes from fid/funcs.txt
PORTED = {
    0x203fb20: 'Main_ScopedFadeCheck',
    0x204b0c0: 'Top_Run',                       # Mod204af08_RunGameStateMachine
    0x204afdc: 'Top_StepBootOverlays',          # Mod204af08_StepOverlayLoadSequence
    0x204a0c4: 'Scene_Run',                     # Mod2049a04_RunMainStateMachine
    0x2074600: 'SaveSelect_Run',                # SaveSelect_ProcessScreen
    0x209f4cc: 'SceneLoopA_Init',
    0x209f50c: 'SceneLoopA_Run',
    0x209f618: 'SceneLoopB_Init',
    0x209f658: 'SceneLoopB_Run',
    0x2089d8c: 'Screen_Request',                # Sprite_UpdateAnimationFrame
    0x2089e04: 'Screen_Run',                    # Sprite_DispatchFrameState
    0x208a2b4: 'Screen_Transition',             # Sprite_SelectContextMode
    0x208a994: 'Screen_LoadOverlay',            # Sprite_SelectRenderState
    0x208abd4: 'Screen_UnloadOverlay',          # Sprite_SelectTextState
}


def sizes():
    """function extents: up to the next function's start (fid/funcs.txt's sizes cut some functions short:
    SaveSelect_ProcessScreen runs to 0x2076c50, its listed size ends at 0x2076038)"""
    starts = sorted(int(l.split()[0], 16) for l in open(os.path.join(ROOT, 'fid/funcs.txt')))
    return {a: b - a for a, b in zip(starts, starts[1:])}


def call_sites():
    """[(site, target or None for BLX register, register number)] inside the ported machines"""
    from capstone import Cs, CS_ARCH_ARM, CS_MODE_ARM
    md = Cs(CS_ARCH_ARM, CS_MODE_ARM)
    md.skipdata = True
    sz = sizes()
    base, img = G._image(None)
    out = []
    for start in PORTED:
        n = sz[start]
        code = bytes(img[start - base:start - base + n])
        for ins in md.disasm(code, start):
            if ins.mnemonic in ('bl', 'blx') and ins.op_str.startswith('#'):
                out.append((ins.address, int(ins.op_str[1:], 0), None))
            elif ins.mnemonic.startswith('blx') and ins.op_str.startswith('r'):
                out.append((ins.address, None, int(ins.op_str[1:])))
    return out


def mem_sites():
    """[(addr, kind 'l'/'s', size, signed, cond, base reg, index reg or None, shift kind, shift amount, disp,
    subtract, post-indexed, data reg)] for the loads / stores inside the ported machines (sp- and pc-based excluded;
    LDM / STM listed with size 4 * count)"""
    from capstone import Cs, CS_ARCH_ARM, CS_MODE_ARM, arm_const as A
    md = Cs(CS_ARCH_ARM, CS_MODE_ARM)
    md.skipdata = True
    md.detail = True
    sz = sizes()
    base, img = G._image(None)
    out = []
    regno = lambda r: int(md.reg_name(r)[1:]) if md.reg_name(r)[:1] == 'r' else {'sb': 9, 'sl': 10, 'fp': 11, 'ip': 12, 'sp': 13, 'lr': 14, 'pc': 15}[md.reg_name(r)]
    for start in PORTED:
        code = bytes(img[start - base:start - base + sz[start]])
        for ins in md.disasm(code, start):
            mn = ins.mnemonic
            if mn == '.byte':
                continue
            stem = mn[:-2] if ins.cc != A.ARM_CC_AL and len(mn) > 2 else mn
            if stem in ('ldr', 'ldrb', 'ldrh', 'ldrsb', 'ldrsh', 'str', 'strb', 'strh'):
                ops = ins.operands
                memi = next(i for i, o in enumerate(ops) if o.type == A.ARM_OP_MEM)
                mo = ops[memi].mem
                b = regno(mo.base)
                if b in (13, 15):
                    continue
                idx = regno(mo.index) if mo.index else None
                post = len(ops) > memi + 1
                disp = mo.disp
                sub = ops[memi].subtracted
                shk, sha = ops[memi].shift.type, ops[memi].shift.value
                if post:
                    disp, idx, sub = 0, None, False
                size = 1 if stem.endswith('b') else 2 if stem.endswith('h') else 4
                out.append((ins.address, 'l' if stem.startswith('ldr') else 's', size, 's' in stem[3:], ins.cc, b, idx,
                            shk, sha, disp, sub, post, regno(ops[0].reg)))
            elif stem[:3] in ('ldm', 'stm') and not stem.startswith(('push', 'pop')):
                ops = ins.operands
                b = regno(ops[0].reg)
                if b == 13:
                    continue
                regs = [regno(o.reg) for o in ops[1:]]
                mode = stem[3:] or 'ia'
                out.append((ins.address, 'L' if stem.startswith('ldm') else 'S', 4 * len(regs), False, ins.cc, b, None,
                            0, 0, mode, False, False, regs))
    return out


def cond_ok(cc, cpsr):
    from capstone import arm_const as A
    n, z, c, v = (cpsr >> 31) & 1, (cpsr >> 30) & 1, (cpsr >> 29) & 1, (cpsr >> 28) & 1
    return {A.ARM_CC_EQ: z, A.ARM_CC_NE: not z, A.ARM_CC_HS: c, A.ARM_CC_LO: not c, A.ARM_CC_MI: n,
            A.ARM_CC_PL: not n, A.ARM_CC_VS: v, A.ARM_CC_VC: not v, A.ARM_CC_HI: c and not z, A.ARM_CC_LS: not c or z,
            A.ARM_CC_GE: n == v, A.ARM_CC_LT: n != v, A.ARM_CC_GT: not z and n == v,
            A.ARM_CC_LE: z or n != v}.get(cc, True)


def main(session, out_path, last=None):
    from emu_driver import Emu
    recs = [json.loads(l) for l in open(session)]
    start = next(r for r in recs if r['ev'] == 'start')['state']
    if not start.endswith('.dst'):
        start = 'tools/emu_scripts/states/%s.dst' % start
    e = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), os.path.join(ROOT, start))
    e.pin_rtc()
    f = open(out_path, 'w')
    install(e, f)
    inputs = {r['f']: (r['mask'], r['touch']) for r in recs if r['ev'] == 'in'}
    end = last or max(r['f'] for r in recs)
    touching = False
    while e.frames < end:
        fr = e.frames
        if fr in inputs:
            mask, touch = inputs[fr]
            e.e.input.keypad_update(mask)
            if touch:
                e.e.input.touch_set_pos(touch[0], touch[1]); touching = True
            elif touching:
                e.e.input.touch_release(); touching = False
        e.step()
    f.close()


def install(e, f, pre=None):
    """hook the ported machines' calls and memory accesses on emulator `e`, writing records to file `f`.
    pre: {address: [fn()]} run first at that address (py-desmume keeps one hook per address: everything that hooks
    an address the recorder uses must go through here)"""
    m = e.e.memory
    reg = m.register_arm9
    open_calls = {}
    sites = call_sites()

    M = 0xffffffff
    U = m.unsigned

    def mk_call(site, target, rn):
        def on_call():
            t = target if target is not None else getattr(reg, 'r%d' % rn)
            args = (reg.r0 & M, reg.r1 & M, reg.r2 & M, reg.r3 & M)
            f.write('c %x %x %x %x %x %x\n' % ((site, t & M) + args))
            open_calls.setdefault(site, []).append(args)

        def on_ret():
            if open_calls.get(site):
                args = open_calls[site].pop()
                f.write('r %x %x\n' % (site, reg.r0 & M))
                for k, a in enumerate(args):
                    if 0x27e0000 <= a < 0x27e3ff0:
                        f.write('o %x %d %s\n' % (site, k, ' '.join('%x' % U.read_long(a + 4 * i) for i in range(4))))
        return on_call, on_ret

    # one hook per address: the return of a call and the next call can share an address (consecutive BLs);
    # the return is handled first
    at = {}
    for site, target, rn in sites:
        c, r = mk_call(site, target, rn)
        at.setdefault(site + 4, []).append(r)
        at.setdefault(site, []).append(c)
    def rd(a, size):
        return U.read_byte(a) if size == 1 else U.read_short(a) if size == 2 else U.read_long(a)

    def mk_mem(addr, kind, size, signed, cc, b, idx, shk, sha, disp, sub, post, rt):
        def on_mem():
            if not cond_ok(cc, reg.cpsr & M):
                return
            base_v = reg[b] & M
            if kind in 'ls':
                off = disp
                if idx is not None:
                    iv = reg[idx] & M
                    if sha:
                        from capstone import arm_const as A
                        iv = {A.ARM_SFT_LSL: iv << sha, A.ARM_SFT_LSR: iv >> sha,
                              A.ARM_SFT_ASR: (iv >> sha) | ((M << (32 - sha)) if iv >> 31 else 0),
                              A.ARM_SFT_ROR: (iv >> sha) | (iv << (32 - sha))}[shk] & M
                    off = iv
                a = (base_v - abs(off) if sub else base_v + off) & M
                if kind == 'l':
                    f.write('l %x %d %x\n' % (a, size, rd(a & ~(size - 1), size)))
                else:
                    f.write('s %x %d %x\n' % (a, size, (reg[rt] & M) & ((1 << (8 * size)) - 1)))
            else:
                n = len(rt)
                a = {'ia': base_v, 'ib': base_v + 4, 'da': base_v - 4 * n + 4, 'db': base_v - 4 * n}[disp] & M
                for i, r in enumerate(rt):
                    if kind == 'L':
                        f.write('l %x 4 %x\n' % (a + 4 * i, rd(a + 4 * i, 4)))
                    else:
                        f.write('s %x 4 %x\n' % (a + 4 * i, reg[r] & M))
        return on_mem

    for t in mem_sites():
        at.setdefault(t[0], []).append(mk_mem(*t))
    for addr, fns in (pre or {}).items():
        for fn in fns:
            fn.__name__ = 'pre'
        at.setdefault(addr, [])[:0] = fns
    d = U.read_long(0x2139cdc)
    f.write('S %x %s\n' % (d, ' '.join('%x' % U.read_long(0x2139ce0 + 4 * i) for i in range(15))))
    for addr, fns in at.items():
        fns.sort(key=lambda fn: {'pre': 0, 'on_ret': 1}.get(fn.__name__, 2))

        def hook(a, s, fns=fns):
            for fn in fns:
                fn()
            return True
        m.register_exec(addr, hook)
    # the entry the game's frame loop makes into the flow (GameHeap_UpdateGameState -> Main_ScopedFadeCheck)
    # with a RAM snapshot at the first frame boundary (OUT.txt.ram: main RAM 0x2000000..0x2400000), so a replay can
    # start the engine's machine from the game's exact state (engine/tests/runtime_live.rs)
    first = [True]

    def on_frame(a, s):
        if first[0] and hasattr(f, 'name'):
            first[0] = False
            with open(f.name + '.ram', 'wb') as g:
                g.write(bytes(e.ram()))
        f.write('f %x\n' % e.frames)
        return True
    m.register_exec(0x203fb20, on_frame)


if __name__ == '__main__':
    if sys.argv[1] == '--sites':
        s = call_sites()
        print(len(s), 'call sites;', sum(1 for x in s if x[1] is None), 'register calls;', len(mem_sites()), 'loads / stores')
    else:
        main(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else None)
