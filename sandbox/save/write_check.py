"""End-to-end writer proof, our code only: card -> unpack -> edit the GameState image -> pack -> card write; the game then
boots on the new save and loads the edited GameState.
    .venv/bin/python sandbox/save/write_check.py SAVE.sav OUT.sav"""
import os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import savefmt as sf  # noqa: E402
from boot_check import boot  # noqa: E402

GS = 0x2142c8c
H2 = 0x2144250 + 2 * 0x688 - GS          # Nene's heroine record in the GameState image

card = sf.Card.open(sys.argv[1])
img = bytearray(sf.unpack('B', card.read('B')))
assert sf.pack('B', bytes(img)) == card.read('B')
old = img[H2 + 6]
img[H2 + 6] = (old + 37) & 0xff
off, gen = card.write('B', sf.pack('B', bytes(img)))
open(sys.argv[2], 'wb').write(card.image)
print('affection %d -> %d written at %#x generation %d' % (old, img[H2 + 6], off, gen))
assert sf.Card.open(sys.argv[2]).read('B') == sf.pack('B', bytes(img))

emu = boot(sys.argv[2], 0)
m = emu.e.memory.unsigned
for f in range(6000):
    if f % 90 == 45:
        emu.touch(128, 150)
    emu.step()
    if f % 30 == 0:
        ram = bytes(m[GS + i] for i in range(0x3c58)) if False else bytes(emu.ram()[GS - 0x2000000:GS - 0x2000000 + 0x3c58])
        same = sum(a == b for a, b in zip(ram, img))
        if ram[H2 + 6] == img[H2 + 6]:
            print('frame %d: game RAM GameState: affection %d, %d / %d bytes equal to our image' % (f, ram[H2 + 6], same, len(img)))
            diff = [hex(i) for i in range(len(img)) if ram[i] != img[i]]
            print('differing:', diff[:20])
            break
else:
    print('not loaded within 6000 frames; affection in RAM', ram[H2 + 6])
