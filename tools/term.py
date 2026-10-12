#!/usr/bin/env python3
"""Parse TERM_BIN condition-list files (tmx_tbin.arc entries, event/check/common.tbin).

Layout (little endian), verified against CondList_CollectPassingRecords/EvaluateGroups (0x206743c/0x2067304):
    +0x00  "TERM_BIN\\0"-style magic (8 bytes)
    +0x08  u32 record count
    +0x0c  12 bytes (opaque header words; not read by the evaluator)
    +0x18  records, each:
        +0x00 u32 event id (script id)         +0x04 u32 priority (a pass only replaces lower-priority results)
        +0x08 u32 used-flag id (scoped bit; written to the output as the "already seen" marker)
        +0x0c u32 n0, +0x10 u32 n1, +0x14 u32 n2   condition counts of the three OR groups
        +0x18 (n0+n1+n2) x { u32 op, u32 a, u32 b }  conditions; op indexes the handler table at 0x21246c8
Evaluation: within a group all conditions with a handler must pass (AND); a group with no handler-backed condition does
not count; the record passes when at least one counted group passes (OR).
"""
import json
import struct
import sys


def parse(data):
    cnt = struct.unpack_from('<I', data, 8)[0]
    pos = 0x18
    recs = []
    for _ in range(cnt):
        rid, prio, flag, n0, n1, n2 = struct.unpack_from('<6I', data, pos)
        pos += 0x18
        groups = []
        for n in (n0, n1, n2):
            g = [struct.unpack_from('<3I', data, pos + 12 * i) for i in range(n)]
            pos += 12 * n
            groups.append(g)
        recs.append({'id': rid, 'priority': prio, 'flag': flag, 'groups': groups})
    return recs, pos


if __name__ == '__main__':
    import os
    import collections
    ops = collections.Counter()
    nrec = 0
    bad = []
    d = sys.argv[1]
    for f in sorted(os.listdir(d)):
        data = open(os.path.join(d, f), 'rb').read()
        try:
            recs, end = parse(data)
        except Exception as e:
            bad.append((f, str(e))); continue
        if end != len(data):
            bad.append((f, 'trailing %d' % (len(data) - end)))
        nrec += len(recs)
        for r in recs:
            for g in r['groups']:
                for op, a, b in g:
                    ops[op] += 1
    print('records', nrec, 'distinct ops', len(ops), 'bad', bad[:5], len(bad))
    print(sorted(ops.items()))
