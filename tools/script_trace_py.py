"""Python side of the Rust VM differential test: same canonical trace as engine/src/bin/script_trace.rs.
usage: script_trace_py.py SCRIPT_DIR ACC_CMDS.txt"""
import os, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
from lpvm import Script

def segs(v):
    return ';'.join(('s:%s' % x if k == 's' else 'v:%d' % x) for k, x in v)

def main():
    src, accf = sys.argv[1:3]
    acc = {int(l, 16) for l in open(accf) if l.strip()}
    out = sys.stdout
    for f in sorted(x for x in os.listdir(src) if x.endswith('.sbin')):
        try:
            sc = Script(dec=open(os.path.join(src, f), 'rb').read())
        except Exception:
            continue
        for e in range(len(sc.entries)):
            g = sc.run(e, max_steps=20000)
            reply = None; count = 0; buf = []
            try:
                while True:
                    ev = g.send(reply); reply = None
                    if ev[0] == 'cmd':
                        buf.append('C %x %s' % (ev[1], ','.join(map(str, ev[2]))))
                        if ev[1] in acc:
                            reply = (((ev[1] * 31 + count * 17 + e) & 0xffffffff) >> 2) & 1
                        count += 1
                    elif ev[0] == 'text':
                        d = ev[1]
                        buf.append('T %s %s|%s|%s' % ('None' if ev[2] is None else 'Some(%d)' % ev[2], segs(d['name']), segs(d['sub']), segs(d['text'])))
                    elif ev[0] == 'wait_fade': buf.append('W')
                    elif ev[0] == 'end': buf.append('E %d' % ev[1]); break
                    elif ev[0] == 'limit': buf.append('L %d' % ev[1]); break
                    elif ev[0] == 'badop': buf.append('B %d %d' % (ev[1], ev[2])); break
            except (IndexError, struct.error):
                buf.append('F')
            out.write('== %s %d\n%s\n' % (f, e, '\n'.join(buf)))
main()
