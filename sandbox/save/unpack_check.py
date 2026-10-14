"""Prove the loader side with the game's own code: unpack a packed block (ours) into the game's struct under Unicorn.
    .venv/bin/python sandbox/save/unpack_check.py STATE.dst SAVE.sav"""
import json, os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import packemu, savefmt as sf  # noqa: E402
from unicorn.arm_const import UC_ARM_REG_R0  # noqa: E402

GS, GS_SIZE = 0x2142c8c, 0x3c58


def unpack_b(pe, packed):
    """run Ov00Init_UnpackSaveBFromBuffer on `packed`; returns the game's unpacked GameState image (before the commit)"""
    io = pe._alloc(len(packed) + 0x20)
    pe.uc.mem_write(io, packed)
    pe.uc.mem_write(0x2166a84 + 8, struct.pack('<I', io))
    got = {}

    def commit(uc):                                     # 0x2063274(temp): copy the unpacked state into GameState
        got['tmp'] = pe.rd(uc.reg_read(UC_ARM_REG_R0), GS_SIZE)
        pe._ret(uc, 0)
    pe.stub(0x2063274, commit)
    pe.call('Ov00Init_UnpackSaveBFromBuffer')
    return got['tmp']


if __name__ == '__main__':
    pe = packemu.PackEmu(sys.argv[1])
    ram_gs = pe.rd(GS, GS_SIZE)
    card = sf.Card.open(sys.argv[2])
    packed = card.read('B')
    tmp = unpack_b(pe, packed)
    same = sum(a == b for a, b in zip(tmp, ram_gs))
    diff = [i for i in range(GS_SIZE) if tmp[i] != ram_gs[i]]
    print('game unpack of the save vs RAM GameState: %d / %d bytes equal; differing offsets: %d (first %s)' % (same, GS_SIZE, len(diff), [hex(x) for x in diff[:12]]))
    # repack what the game unpacked: must give the save's block back
    pe2 = packemu.PackEmu(main=pe.main0[:GS - 0x2000000] + tmp + pe.main0[GS - 0x2000000 + GS_SIZE:], dtcm=pe.dtcm0)
    again, _, _ = pe2.pack('B')
    print('pack(unpack(save)) == save block:', again == packed)
    # change Nene's affection through the save map and let the game unpack it
    m = json.load(open(os.path.join(HERE, 'savemap.json')))
    loc = next((s, e) for s, sec in enumerate(m['B']['sections']) for e, ent in enumerate(sec) if ent.get('src') == 'heroine2+0x6')
    secs = sf.parse('B', packed)
    old = secs[loc[0]][loc[1]][0]
    secs[loc[0]][loc[1]] = bytes([(old + 37) & 0xff])
    tmp2 = unpack_b(pe, sf.serialize('B', secs))
    off = 0x2144250 + 2 * 0x688 + 6 - GS
    print('heroine2 +6 via entry %s: %d -> unpacked by the game as %d' % (loc, old, tmp2[off]))
    print('other bytes unchanged:', sum(1 for i in range(GS_SIZE) if i != off and tmp2[i] != tmp[i]) == 0)
