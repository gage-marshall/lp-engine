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
    1: 'always true (handler is `mov r0,#1; bx lr`): a placeholder condition',
    2: 'scoped flag `a` is SET', 13: 'scoped flag `a` is SET', 24: 'scoped flag `a` is SET', 107: 'scoped flag `a` is SET',
    3: 'scoped flag `a` is CLEAR', 14: 'scoped flag `a` is CLEAR', 25: 'scoped flag `a` is CLEAR', 108: 'scoped flag `a` is CLEAR',
    4: 'scoped byte `a` == b', 5: 'scoped byte `a` <= b', 6: 'scoped byte `a` >= b',
    15: 'scoped byte `a` == b', 16: 'scoped byte `a` <= b', 17: 'scoped byte `a` >= b',
    26: 'scoped byte `a` == b', 27: 'scoped byte `a` <= b', 28: 'scoped byte `a` >= b',
    40: 'Manaka (slot 0) relationship phase (record +4) == a', 41: 'Nene (slot 2) phase == a', 42: 'Rinko (slot 1) phase == a',
    43: 'Manaka phase parity: a=0 phase in {1,3}, a=1 phase in {0,2}', 44: 'Nene phase parity (same sets)', 45: 'Rinko phase parity (same sets)',
    46: 'Manaka phase half: a=0 phase in {0,1}, a=1 phase in {2,3}', 47: 'Nene phase half (same sets)', 48: 'Rinko phase half (same sets)',
    49: 'season == a (a 0..3; a=4: seasons 1,3; a=5: seasons 0,2)', 50: 'season != a (same grouping)',
    51: 'time period / hour window a holds (a 0..5 = period; 6..12 = hour windows)', 52: 'negation of 51',
    53: 'month == a', 54: 'month != a',
    55: 'calendar event a starts within the next 30 days, not today', 56: 'calendar event a is active today',
    57: 'calendar event a ended within the last 30 days, not today', 58: 'calendar event a is NOT active today',
    153: 'calendar event a starts within the next 7 days, not today', 154: 'calendar event a starts within the next 14 days, not today',
    59: 'rank of player stat 0 (exercise) == a', 60: 'rank of exercise <= a', 61: 'rank of exercise >= a',
    62: 'rank of knowledge == a', 63: 'rank of knowledge <= a', 64: 'rank of knowledge >= a',
    65: 'rank of sense == a', 66: 'rank of sense <= a', 67: 'rank of sense >= a',
    68: 'rank of charm == a', 69: 'rank of charm <= a', 70: 'rank of charm >= a',
    71: 'current slot state value (SaveSlot_CallStateHook) == a', 72: 'current slot state value <= a', 73: 'current slot state value >= a',
    74: 'GameClock_GetCurrentSlot(0x20, current slot) bit 0 (per-slot flag 0x20)', 75: 'GameClock_GetCurrentSlot(4, current slot) bit 0 (per-slot flag 4)',
    77: 'hearts level (GetLevelFromPoints) + 1 >= b', 114: 'hearts level + 1 <= b',
    78: 'random chance, rule a: 0: main-game and 10%; 1: main-game and 35%; 2: main-game and not E0, else 70%; 3: always; 4-5: never; 6: main-game, else slot-dependent percent table 30/15/5/0/2; 7: main-game and 10%; 8, 10, 11: b percent; 9: 30%',
    79: 'slot table value (kind 0) of mapped id(a) >= b', 147: 'slot table value (kind 1) of mapped id(a) >= b',
    80: 'current entry byte equals that of mapped id(a)',
    82: 'weeks elapsed: (GameState+0x9c - +0x130) >= b*7',
    83: 'time-struct rule a matches the planned activity code (StateLogic_SelectDataUpsilon)',
    85: 'per-slot flag 1 of the current slot == a',
    87: 'slot table getter (kind 2) of entry a >= b', 88: 'slot table getter (kind 3) of entry a >= b', 89: 'slot table getter (kind 2) of entry a >= b',
    90: 'all ids of the fixed 20-entry list are non-zero (slot table kind 0)',
    91: 'today is a special date (5/27, 10/13, 2/9, 7/19, 12/4, 2/11, 3/9)',
    92: 'slot index wrapper >= b', 94: 'slot table kind-3 getter 0x5e >= b', 96: 'stat block byte 0x73 >= b', 97: 'max counter +0x3a1b >= b',
    111: 'the planned activity code (time-struct byte 5) == a',
    112: 'counter +0xa0 is above 98',
    116: 'weekday group a (0..5 = Mon..Sat, 6 = Sunday, 7..10 = workday/holiday groups using calendar event 0x51)', 117: 'negation of 116',
    118: 'current slot progress (SaveSlot_ResetFromField8) >= a + 1',
    131: 'stat block byte 0x70/0x71/0x72 (a = 0/1/2) >= b', 132: 'stat block byte 0x74 >= b (a must be 0)',
    133: 'sum of save-record signed bytes 0x49,0x4a,0x4b >= b', 134: 'bit-flag array byte 0x600 >= b',
    136: 'player exercise <= b', 140: 'player knowledge >= b', 145: 'player charm <= b', 146: 'player charm >= b',
    148: 'daily-shuffled assignment of the current slot == a (RTC-day LCG shuffle of 4 entries, tables [4,3,2,1])',
    152: 'EventLogic record a/2 is assigned (odd a negates)',
    155: 'now is inside date/time window rule a (CalendarRule_ResolveDateTimeForClock, bit 7 of the rule record)',
    156: 'current slot level (SaveSlot_GetLevel) >= b',
    157: 'current slot has the highest level (ties by +5 byte)',
    158: 'save-record year == current year (a=0 negates)',
    159: 'week/holiday lookup (3 kinds by a) equals table[a]',
    170: 'main-game mode and flag E0 clear == a',
    178: 'average of the four player stats >= b',
    179: 'per-slot flag 2 of the active slot == a',
    180: 'planned-id category (Mod2057e7c byte 1 of field 4) satisfies one of the scoped flags 0xea4..0xea7',
    185: 'field 4 value (300..308) does NOT map to category a',
    188: 'counter +0x134 <= b', 189: 'counter +0x134 >= b',
    210: 'slot-table getter 0x170 > 4',
    211: 'counter (StateLogic_GetData_cc834 / cc88c, selected by a=0/1) >= b',
    213: 'schedule entry committed and no low-five flags == (a != 0)',
    214: 'no low-five schedule flags (a = 0) / low-five flags present (a != 0)',
    215: 'schedule timer cmp(a) b', 216: 'schedule entry flag cmp(a) b',
    217: 'counter 0x2cd cmp(a) b', 218: 'counter 0x2ce cmp(a) b', 219: 'counter 0x2cf cmp(a) b', 220: 'counter 0x2d0 cmp(a) b',
    221: 'counter table[0] cmp(a) b', 222: 'counter table[1] cmp(a) b', 223: 'counter table[2] cmp(a) b', 224: 'counter table[3] cmp(a) b',
    225: 'game-flags hook 0x2057a24 clear (a=0) / set (a=1)', 226: 'upcoming schedule entry == 0xa7 (a=0) / != 0xa7 (a=1)',
    229: 'scoped flag 0xed0 clear (a=0) / set (a=1)', 230: 'counter byte +0x2e3 cmp(a) b',
    232: 'level table value (0/1/2) cmp(a) b',
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
             '**Scoped flag ids** (script cmds 0x55/0x59, TERM used-flags, ops 2/3/13/14/24/25/107/108, 229): `scope = (id >> 12) & 3` (0 = global flags, 1/2/3 = the per-heroine flag set of slot 0/1/2 = Manaka/Rinko/Nene), bit index `= (id & 0xfff) | ((id & 0x4000) >> 2)` in a 0x800-byte bit array that is swapped per scope (`GameFlags_TestScopedBit` 0x209a088 -> `NormalizeId`, `EnterScopeForRead`, `TestBit`); scoped *bytes* (ops 4-6, 15-17, 26-28, cmd 0x107) live in the same storage at byte offset 0x800 + index. The used-flags of the 2,920 records are 979 / 949 / 986 heroine-scoped (Manaka / Rinko / Nene) and 6 global.', '',
             'Operand conventions: `a`, `b` are the 2nd/3rd condition words. cmp(a) b: a=0 `value >= b`, a=1 `==`, a=2 `<=`.  The caller combines results as `acc &= (r0 & 1)` for every condition of a group, groups are OR-ed, and a group with no handler-backed condition does not count.', '',
             'Verification: `tools/cond_check.py` runs the real ARM handler of every op against `tools/cond_eval.py` for every (op, a, b) in the data over 40 mocked game states (all 107 ops bit-exact), and `tools/cond_check_files.py` does the same for whole files through native `CondList_CollectPassingRecords` (6,612 evaluations, 0 mismatches; the mock replaces every leaf outside the module with a deterministic function of its arguments).\n\nCross-check: for %s of the records the event\'s own script sets (cmd 0x55) the record\'s used-flag, confirming the record layout and the meaning of `flag`.' % ('%d of %d' % flag_validation(os.path.dirname(HERE)) if flag_validation(os.path.dirname(HERE)) else 'n/a'), '',
             'Described ops cover %.1f%% of all %d conditions.' % (100.0 * known / total, total), '',
             '| op | uses | meaning |', '|---|---|---|']
    for op in sorted(counts, key=lambda o: -counts[o]):
        lines.append('| %d (0x%x) | %d | %s |' % (op, op, counts[op], OPS.get(op, '**undecoded**')))
    open(os.path.join(root, 'docs/CONDITIONS.md'), 'w').write('\n'.join(lines) + '\n')
    print('ops', len(counts), 'covered %.1f%%' % (100.0 * known / total), 'undescribed', sorted(o for o in counts if o not in OPS))


if __name__ == '__main__':
    main()
