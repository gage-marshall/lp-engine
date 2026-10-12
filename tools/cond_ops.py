#!/usr/bin/env python3
"""Condition-op vocabulary of the TERM_BIN condition lists (see tools/term.py) and the generator of docs/CONDITIONS.md.

A condition is (op, a, b); `op` indexes the handler table at 0x21246c8 (237 entries; handler(op, a, b) -> bool).
OPS maps op -> (summary, evidence).  Wording uses the game's own state names; 'slot' = the heroine slot the scene is
about (GameState_GetCurrentSlot, 0..2), 'level' = SaveSlot_GetLevel (relationship level from points), 'player stats' =
the four training parameters at GameState +0x2b4..0x2b7 (運動 知識 感性 魅力), 'scoped flag' = GameFlags_TestScopedBit
(ids as used by script cmd 0x55/0x59).
"""
import collections
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from term import parse  # noqa: E402

STATS = ('exercise', 'knowledge', 'sense', 'charm')

OPS = {
    1: 'record-level: the list entry is considered only if the *failing* path is wanted (CondList_CollectFailingRecords thunk; never a leaf test)',
    2: 'scoped flag `a` is SET', 3: 'scoped flag `a` is CLEAR',
    13: 'scoped flag `a` is CLEAR', 14: 'scoped flag `a` is CLEAR (alias of 13)',
    24: 'scoped flag `a` is SET', 25: 'scoped flag `a` is CLEAR',
    107: 'scoped flag `a` is SET', 108: 'scoped flag `a` is SET (alias of 107)',
    4: 'scoped byte `a` == b', 5: 'scoped byte `a` <= b', 6: 'scoped byte `a` >= b',
    15: 'scoped byte `a` == b', 16: 'scoped byte `a` <= b', 17: 'scoped byte `a` >= b',
    26: 'scoped byte `a` == b', 27: 'scoped byte `a` <= b', 28: 'scoped byte `a` >= b',
    40: 'GameFlags(0x800) clear', 41: 'GameFlags(0x800) set', 42: 'GameFlags(0x800) set (variant)',
    74: 'game flag test (variant of 40-42)', 75: 'game flag test (variant of 40-42)',
    49: 'season == a (a 0..3; a=4: summer/winter group 1,3; a=5: group 0,2)', 50: 'season != a (same grouping)',
    51: 'time period / hour window `a` holds (0..5 = period 0..5; 6..12 = hour windows, see handler)', 52: 'negation of 51',
    53: 'month == a', 54: 'month != a',
    55: 'calendar event `a` starts within the next 30 days (not today)', 56: 'calendar event `a` is active today or within the window',
    57: 'calendar event `a` happened within the last 30 days (not today)', 58: 'calendar event `a` is NOT active in the window',
    153: 'calendar event `a` within the next 7 days', 154: 'calendar event `a` within the next 14 days',
    61: 'player stat0 (exercise) >= b', 64: 'player stat1 (knowledge) >= b', 66: 'player stat2 (sense) <= b',
    67: 'player stat2 (sense) >= b', 70: 'player stat3 (charm) >= b',
    136: 'player exercise <= b', 140: 'player knowledge >= b', 145: 'player charm <= b', 146: 'player charm >= b',
    71: 'current slot value (SaveSlot_CallStateHook) == a', 72: 'current slot value <= a', 73: 'current slot value >= a',
    77: 'current slot level + 1 >= b', 114: 'current slot level + 1 <= b',
    78: 'random chance: a selects the roll (Rand_Next(100) < 10 / 35 / 70 / 30 / b ... depending on a and game state)',
    79: 'slot table value (kind 0, mapped id of `a`) >= b', 147: 'slot table value (kind 1, mapped id of `a`) >= b',
    80: 'current entry byte equals that of mapped index `a`',
    82: 'weeks elapsed: (GameState+0x9c - GameState+0x130) >= b*7 days',
    83: 'time-struct rule `a` matches (StateLogic_SelectDataUpsilon_cf8e4)',
    85: 'slot flag 1 of the current slot == a',
    87: 'slot table getter (4a/57/48 family, kind 2) >= b', 88: 'slot table getter (kind 3 variant) >= b', 89: 'slot table getter >= b',
    90: 'all ids in a fixed 21-entry list read zero',
    91: 'today is one of the special dates (5/27, 10/13, 2/9, 7/19, 12/4, 2/11, 3/9)',
    92: 'slot index wrapper >= b', 94: 'slot table kind3 getter 0x5e >= b', 96: 'stat block byte 0x73 >= b',
    97: 'max counter GameState+0x3a1b >= b',
    111: 'byte 4 of the time struct (StateLogic_UpdateTimeDataChi) == a',
    112: 'GameState counter at +0xa0 is above 98',
    116: 'weekday == a (0..6; 7..9 weekend / workday groups using calendar event 0x51)', 117: 'weekday != a (negated 116)',
    118: 'current slot progress (SaveSlot_ResetFromField8) >= a + 1',
    131: 'stat block byte 0x70/0x71/0x72 (a = 0/1/2) >= b', 132: 'stat block byte 0x74 >= b (a must be 0)',
    133: 'sum of save-slot signed bytes 0x49,0x4a,0x4b >= b', 134: 'bit-flag array byte 0x600 >= b',
    148: 'today\'s daily-shuffled heroine slot == a (RTC day LCG shuffle of 4 entries)',
    152: 'EventLogic record a is assigned (a odd = negated)',
    155: 'now is inside date/time window rule a (CalendarRule_ResolveDateTimeForClock, table of 5/5/5 windows)',
    156: 'date/time window rule a (variant)',
    157: 'current slot has the highest level (ties broken by points)',
    158: 'save year == current year (a = 0 negates)',
    159: 'holiday/week-table lookup equals table[a]',
    170: 'main-game mode active and flag 0xe0 clear == a',
    178: 'average of the four player stats >= b (op 0xb2)',
    179: 'slot flag 2 of the active slot == a',
    180: 'current id category (Mod2057e7c entry byte 1) satisfies a scoped-bit test',
    185: 'current entry value (GameFlags field 4) maps to category a (0xb9 negates)',
    188: 'counter GameState+0x134 <= b', 189: 'counter GameState+0x134 >= b',
    210: 'slot-table getter 0x170 > 4',
    211: 'counter (StateLogic_GetData_cc834/cc88c selected by a) >= b',
    213: 'schedule entry active and no low-five flags == a',
    214: 'no low-five schedule flags (a=0) / low-five schedule flags (a!=0)',
    215: 'schedule timer (a: 0 >=, 1 ==, 2 <=) b', 216: 'schedule entry flag (a: 0 >=, 1 ==, 2 <=) b',
    217: 'counter 0x2cd (a: cmp) b', 218: 'counter 0x2ce (a: cmp) b', 219: 'counter 0x2cf (a: cmp) b', 220: 'counter 0x2d0 (a: cmp) b',
    221: 'counter table index 0 (a: cmp) b', 222: 'counter table index 1 (a: cmp) b', 223: 'counter table index 2 (a: cmp) b', 224: 'counter table index 3 (a: cmp) b',
    225: 'game-flags thunk 0x2057a24 set', 226: 'schedule entry value == 0xa7 (a=0) / != 0xa7',
    229: 'scoped flag 0xed0 set', 230: 'save-slot value (Get20cb4e0) compared with b',
    232: 'level table value compared with b',
}


def stats(term_dir):
    counts = collections.Counter()
    for f in sorted(os.listdir(term_dir)):
        recs, _ = parse(open(os.path.join(term_dir, f), 'rb').read())
        for r in recs:
            for g in r['groups']:
                for op, a, b in g:
                    counts[op] += 1
    return counts


def flag_validation(root):
    """Cross-check: does the script of a record set (cmd 0x55) the record's own used-flag?"""
    d = json.load(open(os.path.join(root, 'content/scripts/directory.json')))['records']
    slots = json.load(open(os.path.join(root, 'content/scripts/resource_map.json')))['slots']
    setf = collections.defaultdict(set)
    path = os.path.join(root, 'fid/script_callsites.jsonl')
    if not os.path.exists(path):
        return None
    for line in open(path):
        x = json.loads(line)
        if x['cmd'] == 0x55 and x['args']:
            setf[x['f']].add(x['args'][0])
    tdir = os.path.join(root, 'work/assets/arc/tmx_tbin.arc')
    tot = ok = 0
    for f in sorted(os.listdir(tdir)):
        recs, _ = parse(open(os.path.join(tdir, f), 'rb').read())
        for r in recs:
            ref = d.get('%08x' % r['id'])
            ent = slots.get(str(ref['slot']), {}).get('entries') if ref else None
            if not ent or ref['script'] >= len(ent):
                continue
            tot += 1
            ok += r['flag'] in setf.get('%04d.sbin' % ent[ref['script']], ())
    return ok, tot


def main():
    root = os.path.dirname(HERE)
    counts = stats(os.path.join(root, 'work/assets/arc/tmx_tbin.arc'))
    total = sum(counts.values())
    known = sum(n for op, n in counts.items() if op in OPS)
    lines = ['# Condition lists (TERM_BIN) and the condition-op vocabulary', '',
             'Generated by `tools/cond_ops.py` from `tmx_tbin.arc` (688 files, 2,920 records, %d conditions, %d distinct ops).' % (total, len(counts)), '',
             'Native code: `MapArc_LoadMergedEntryList` (0x206febc) opens `arc/tmx_tbin.arc`, reads each listed entry and calls `CondList_CollectPassingRecords` (0x206743c). '
             'Format and evaluation are documented in `tools/term.py`. A record is `{event id, priority, used-flag, 3 OR-groups of (op, a, b) conditions}`; '
             'the evaluator keeps only records whose priority is >= the best so far and, for each passing record, outputs `(event id, flag-used)`. '
             'Selection among the survivors is random among unused ones (`MapQueue_SelectRandomEntry`: shuffle, take the first whose used-flag is 0, else a random one that differs from the last picked).', '',
             'Operand conventions: `a`, `b` are the 2nd/3rd condition words. Comparators named cmp: a=0 `>=`, 1 `==`, 2 `<=` unless stated.', '',
             'Cross-check: for %s of the records the event\'s own script sets (cmd 0x55) the record\'s used-flag, confirming the record layout and the meaning of `flag`.' % ('%d of %d' % flag_validation(os.path.dirname(HERE)) if flag_validation(os.path.dirname(HERE)) else 'n/a'), '',
             'Described ops cover %.1f%% of all %d conditions.' % (100.0 * known / total, total), '',
             '| op | uses | meaning |', '|---|---|---|']
    for op in sorted(counts, key=lambda o: -counts[o]):
        lines.append('| %d (0x%x) | %d | %s |' % (op, op, counts[op], OPS.get(op, '**undecoded**')))
    open(os.path.join(root, 'docs/CONDITIONS.md'), 'w').write('\n'.join(lines) + '\n')
    print('ops', len(counts), 'covered %.1f%%' % (100.0 * known / total), 'undescribed', sorted(o for o in counts if o not in OPS))


if __name__ == '__main__':
    main()
