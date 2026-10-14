"""The game's own `Save_WriteBlock` 0x20f0fb8 under Unicorn vs savefmt.Card.write, for every kind and several slots:
card offset, copy / generation choice (from the game's tables 0x2157fb0 / 0x2158020), header, LZ, SHA-1.
    .venv/bin/python sandbox/save/writeblock_check.py STATE.dst"""
import os, random, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import packemu, savefmt as sf  # noqa: E402
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_SP  # noqa: E402

GEN_INDEX = {'A': lambda s: 0, 'B': lambda s: 1 + s, 'C': lambda s: 4, 'D': lambda s: 5, 'E': lambda s: 6 + s, 'F': lambda s: 0x6a + s}
pe = packemu.PackEmu(sys.argv[1])
rng = random.Random(5)
ok = bad = 0
for kind in (sys.argv[2] if len(sys.argv) > 2 else "ABCDEF"):
    slots = sorted({0, sf.KINDS[kind][5] - 1, rng.randrange(sf.KINDS[kind][5])})
    for slot in slots:
        for copy0, gen0 in ((0, 0), (1, 5), (0, 0xffff)):
            pe.reset()
            n = sf._maps()[kind]['packed_size']
            img = bytes(rng.getrandbits(8) if rng.random() < 0.3 else 0 for _ in range(sf._maps()[kind]['struct_size']))
            packed = sf.pack(kind, sf.unpack(kind, sf.pack(kind, img)))          # a canonical block
            buf = pe._alloc(n)
            pe.uc.mem_write(buf, packed)
            gi = GEN_INDEX[kind](slot)
            pe.uc.mem_write(0x2157fb0 + gi, bytes([copy0]))
            pe.uc.mem_write(0x2158020 + 4 * gi, struct.pack('<I', gen0))
            got = {}

            def card_write(uc):
                got['off'] = uc.reg_read(UC_ARM_REG_R1)
                got['data'] = pe.rd(uc.reg_read(UC_ARM_REG_R2), uc.reg_read(UC_ARM_REG_R3))
                pe._ret(uc, 1)
            pe.stub(0x203f420, lambda uc: pe._ret(uc, 0))                      # Card_GetState
            pe.stub(0x203f7c4, card_write)                                       # Card_WriteBackupAsync
            pe.stub(0x203f908, card_write)                                       # Card_WriteBackupRangeAsync
            sp = packemu.STACK - 0x100
            pe.uc.mem_write(sp, struct.pack('<I', n))                            # 5th argument: size
            uc = pe.uc
            for r, v in zip((UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3), ('ABCDEF'.index(kind), slot, buf, 1)):
                uc.reg_write(r, v)
            uc.reg_write(UC_ARM_REG_SP, sp)
            uc.reg_write(packemu.UC_ARM_REG_LR, packemu.STOP)
            uc.emu_start(0x20f0fb8, packemu.STOP, count=20_000_000_000)
            # ours: a card whose current copy / generation are the table's
            card = sf.Card(bytes([0xff]) * 0x40000)
            if gen0:
                card.image[sf.KINDS[kind][4](slot, copy0):sf.KINDS[kind][4](slot, copy0) + 0x20 + n] = card.encode_block(packed, gen0)
            off, gen = card.write(kind, packed, slot)
            mine = bytes(card.image[off:off + len(got.get('data', b''))])
            same = got.get('off') == off and got.get('data') == mine
            ok += same; bad += not same
            if not same:
                print('MISMATCH', kind, slot, copy0, gen0, hex(got.get('off', 0)), hex(off), len(got.get('data', b'')))
print('Save_WriteBlock vs savefmt.Card.write: %d exact, %d mismatched' % (ok, bad))
