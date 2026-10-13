"""Python side of the Rust cross-script machine differential test (engine/src/bin/machine_dump.rs prints the same text).
usage: machine_dump.py CONTENT_DIR   (every directory event, TraceHost reply policy)"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lp_paths
from event_trace import TraceMachine

def segs(v):
    return ';'.join(('s:%s' % x if k == 's' else 'v:%d' % x) for k, x in v)

def main(content):
    scripts = lp_paths.scripts_dir(content); directory = lp_paths.load_directory(content); rm = lp_paths.load_resource_map(content)
    out = []
    for key in sorted(directory['records']):
        ref = directory['records'][key]
        out.append('== %s' % key)
        try:
            m = TraceMachine(scripts, directory, rm, ref)
        except Exception:
            out.append('START-ERR'); continue
        gen = m.run(max_steps=20000); flags = set(); reply = None; n = 0
        try:
            while True:
                n += 1
                if n > 3000: out.append('CAP'); break
                ev = gen.send(reply); reply = None
                k = ev[0]
                if k == 'cmd':
                    cid, args = ev[1], ev[2]; r = 0
                    if cid == 0x55 and args: flags.add(args[0])
                    elif cid == 0x59 and args: r = int(args[0] in flags)
                    out.append('C %x %s -> %d' % (cid, ','.join(map(str, args)), r)); reply = r
                elif k == 'text':
                    d = ev[1]; out.append('T %s %s|%s|%s' % ('None' if ev[2] is None else 'Some(%d)' % ev[2], segs(d['name']), segs(d['sub']), segs(d['text'])))
                elif k == 'wait_fade': out.append('W')
                elif k == 'return_by_id': out.append('R %x' % ev[1])
                elif k == 'call': t = ev[2]; out.append('K %x %d/%d/%d' % (ev[1], t['slot'], t['script'], t['entry']))
                elif k == 'call_ref': t = ev[2]; out.append('KR %x %d/%d/%d' % (ev[1], t['slot'], t['script'], t['entry']))
                elif k == 'unresolved_call': out.append('U %x' % ev[1]); break
                elif k == 'end': out.append('E %d' % ev[1]); break
                elif k == 'limit': out.append('L %d' % ev[1]); break
                elif k == 'fault': out.append('F'); break
                elif k == 'badop': out.append('B %d %d' % (ev[1], ev[2])); break
        except (IndexError, KeyError, ValueError, TypeError) as e:
            out.append('F')
    sys.stdout.write('\n'.join(out) + '\n')

main(sys.argv[1])
