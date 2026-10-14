"""Record a call log (calllog.py) while exploring: replay a recorded session to a frame (the explorer's cached prefix
state), or power on with a battery save, then run a script of taps / waits / captures with the flow machines hooked.
    .venv/bin/python sandbox/flow/explore_calls.py SESSION.jsonl FRAME SCRIPT.json OUT_DIR
    .venv/bin/python sandbox/flow/explore_calls.py BACKUP.sav 0 SCRIPT.json OUT_DIR
SCRIPT: the explorer's steps ({"tap": [x, y], "hold": 4, "release": 20}, {"drag": [[x, y], ...]} (2 frames per point,
release 8), {"wait": N}, {"capture": "name"}, {"shot": "name"}, {"backup": "file.sav"} (the battery save now).
OUT_DIR gets calls.txt (check it with check.py) and the captures."""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'sandbox/ui'))
import explore  # noqa: E402
import capture  # noqa: E402
from calllog import install  # noqa: E402

if __name__ == '__main__':
    session, frame, script, out = sys.argv[1], int(sys.argv[2]), json.load(open(sys.argv[3])), sys.argv[4]
    os.makedirs(out, exist_ok=True)
    if session.endswith('.sav'):                       # power on with this battery save (FRAME ignored)
        from emu_driver import Emu
        e = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'))
        e.e.backup.import_file(session)
        e.e.reset()
        e.pin_rtc()
    else:
        e = explore.replay_to(session, frame, os.path.dirname(os.path.abspath(out)))
    f = open(os.path.join(out, 'calls.txt'), 'w')
    reg = e.e.memory.register_arm9
    pre = {}
    for st in script:
        if 'force_ret' in st:
            # the next return from the call at SITE gives VALUE (instead of what the callee returned), once,
            # optionally only when the callee returned WHEN
            site, value = st['force_ret'][:2]
            when = st['force_ret'][2] if len(st['force_ret']) > 2 else None
            todo = [value]

            def force_ret(todo=todo, when=when, site=site):
                orig = reg.r0 & 0xffffffff
                if todo and (when is None or orig == when):
                    v = todo.pop()
                    reg.r0 = v
                    f.write('F %x %x %x\n' % (site, orig, v & 0xffffffff))
            pre.setdefault(site + 4, []).append(force_ret)
    install(e, f, pre)
    for st in script:
        if 'tap' in st:
            x, y = st['tap']
            e.touch(x, y, hold=st.get('hold', 4), release=st.get('release', 20))
        elif 'drag' in st:
            e.drag([tuple(p) for p in st['drag']], hold=2, release=8)
        elif 'wait' in st:
            e.step(st['wait'])
        elif 'backup' in st:
            e.e.backup.export_file(os.path.join(out, st['backup']))
        elif 'capture' in st:
            capture.capture(e, os.path.join(out, st['capture']))
        elif 'force_result' in st:
            # the next screen result posted (0x2088624) becomes this one: as if the player had picked that entry
            want = [st['force_result']]
            reg = e.e.memory.register_arm9

            def force(a, s_, want=want):
                if want:
                    reg.r0 = want.pop()
                return True
            e.e.memory.register_exec(0x2088624, force)
        elif 'press' in st:
            e.press(*st['press'])
        elif 'shot' in st:
            e.shot(os.path.join(out, st['shot'] + '.png'))
    f.close()
