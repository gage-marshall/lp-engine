"""Add the game's own SaveSlot_ProcessStorageStatus(slot) per backup to engine/tests/data/save/oracle.json.
    .venv/bin/python sandbox/save/status_fixture.py STATE.dst"""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE)); sys.path.insert(0, HERE)
import packemu  # noqa: E402
dst = sys.argv[1]
pe = packemu.PackEmu(dst)
st = []
for slot in range(3):
    pe.reset()
    st.append(pe.call(0x20608f4, slot))
path = os.path.join(ROOT, 'engine/tests/data/save/oracle.json')
fx = json.load(open(path))
fx[os.path.basename(dst)[:-4] + '.sav']['fields']['status'] = st
json.dump(fx, open(path, 'w'), indent=1, ensure_ascii=False)
print('status', os.path.basename(dst), st)
