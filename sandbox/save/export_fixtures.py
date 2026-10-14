"""Write engine fixtures for the save port (derived numbers only: digests and field values, no save or ROM bytes):
  engine/data/save_maps.json        <- sandbox/save/bitmaps.json
  engine/tests/data/save/oracle.json   per backup: SHA-1 of the game's own unpack of each block and of the block itself
    .venv/bin/python sandbox/save/export_fixtures.py STATE.dst      (one process per backup: DeSmuME initialises once)"""
import hashlib, json, os, shutil, sys
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE)); sys.path.insert(0, HERE)
import savefmt as sf  # noqa: E402
from bitmap import Oracle  # noqa: E402

shutil.copy(os.path.join(HERE, 'bitmaps.json'), os.path.join(ROOT, 'engine/data/save_maps.json'))
out_dir = os.path.join(ROOT, 'engine/tests/data/save')
os.makedirs(out_dir, exist_ok=True)
path = os.path.join(out_dir, 'oracle.json')
fx = json.load(open(path)) if os.path.exists(path) else {}
dst = sys.argv[1]
orc = Oracle(dst)
card = sf.Card.open(dst[:-4] + '.sav')
e = {}
for kind in 'ABC':
    cur = card.current(kind)
    blk = card.read(kind)
    img = orc.unpack(kind, blk)
    for i in sf._maps()[kind].get('unpack_untouched', []):
        img = img[:i] + b'\0' + img[i + 1:]
    e[kind] = dict(copy=cur[0], generation=cur[2], stored_size=card.header(cur[1])[0],
                   block_sha1=hashlib.sha1(blk).hexdigest(), image_sha1=hashlib.sha1(img).hexdigest())
for slot in range(1, 3):                                # further save files (B slot = GameState +0x7b)
    if card.current('B', slot):
        cur = card.current('B', slot)
        blk = card.read('B', slot)
        e['B%d' % slot] = dict(copy=cur[0], generation=cur[2], stored_size=card.header(cur[1])[0],
                               block_sha1=hashlib.sha1(blk).hexdigest(),
                               image_sha1=hashlib.sha1(orc.unpack('B', blk)).hexdigest())
gs = orc.unpack('B', card.read('B'))
e['fields'] = dict(affection=[gs[0x15c4 + 0x688 * s + 6] for s in range(3)],
                   name_v0=gs[0x18:0x28].split(b'\0')[0].decode('utf8'), name_1b=gs[0x8:0x18].split(b'\0')[0].decode('utf8'))
fx[os.path.basename(dst)[:-4] + '.sav'] = e
json.dump(fx, open(path, 'w'), indent=1, ensure_ascii=False)
print('->', path, os.path.basename(dst))
