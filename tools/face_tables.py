"""Face-expression / lip-sync / lighting data tables from the game's ARM9 static image (written to content/engine/face_tables.json by lp_extract;
the values are derived from the user's ROM and are not stored in the repo).

Semantics (decoded from the game code):
  mood id (0..44)          -> brow variant, mouth row, cheek variant           (table at 0x2125168, 3 bytes per mood)
  mouth row (0..8)         -> 4 mouth-texture variants [col0..col3]             (table at 0x2125144, 4 bytes per row)
  lip-sync viseme v (0..5) -> mode m = next_mode[v]; mouth = row[col[m]]        (v = byte from lipsync.dat; 5 = silence)
  eye texture              -> eye_table[mood][column]; column 2 is the blink-closed variant   (0x2127ae4, 46 rows x 18)
  blink                    -> interval/hold tick tables cycled by a counter     (0x2103554 / 0x210355e)
  lights                   -> 4 light colours (RGB555) used with the 3 camera-preset light angles (0x2102bac)
"""
import struct

BASE = 0x2000000
N_MOODS, N_ROWS, EYE_COLS = 45, 9, 18


def _s8(b): return [x if x < 128 else x - 256 for x in b]


def extract(arm9):
    def at(a, n): return arm9[a - BASE:a - BASE + n]
    moods = []
    for i in range(N_MOODS):
        a, m, c = _s8(at(0x2125168 + 3 * i, 3)); moods.append({'brow': a, 'mouth_row': m, 'cheek': c})
    rows = [_s8(at(0x2125144 + 4 * i, 4)) for i in range(N_ROWS)]
    eye = [_s8(at(0x2127ae4 + EYE_COLS * i, EYE_COLS)) for i in range(N_MOODS)]
    return {
        'moods': moods, 'mouth_rows': rows, 'eye_table': eye,
        'viseme': {'col': [0, 1, 2, 3, 2, 3], 'next_mode': [1, 2, 3, 4, 5, 0], 'silence': 5},
        'lip_gate': {'peak_threshold': 1000, 'note': 'mouth closes (viseme 5) while the voice stream peak (abs 16-bit sample) is below this'},
        'blink': {'interval': _s8(at(0x2103554, 10)), 'hold': _s8(at(0x210355e, 10)), 'closed_column': 2},
        'light_colors': list(struct.unpack('<4H', at(0x2102bac, 8))),
    }


def mouth_variant(tables, mood, viseme):
    """mouth texture variant for a mood id and a lip-sync viseme (0..5)"""
    row = tables['mouth_rows'][tables['moods'][mood]['mouth_row']]
    m = tables['viseme']['next_mode'][viseme]
    return row[tables['viseme']['col'][m]]


def face_for(tables, mood, viseme=5, column=0, blink=False):
    """-> {'eye','brow','cheek','mouth'} variant numbers"""
    mo = tables['moods'][mood]; eye = tables['eye_table'][mood][tables['blink']['closed_column'] if blink else column]
    return {'eye': eye, 'brow': mo['brow'], 'cheek': mo['cheek'], 'mouth': mouth_variant(tables, mood, viseme)}


def blink_closed(tables, tick):
    """Reference blink cycle: the eye is open for interval[i] ticks, then closed for hold[i] ticks, i cycling through the 10-entry tables
    (the game's state machine does this per logic tick; tick rate assumed 30/s)."""
    iv, ho = tables['blink']['interval'], tables['blink']['hold']; period = sum(iv) + sum(ho); t = tick % period
    for a, b in zip(iv, ho):
        if t < a: return False
        t -= a
        if t < b: return True
        t -= b
    return False


def lip_viseme(keys, t, duration, peak=None, gate=1000):
    """viseme (0..5) at time t seconds of a voice clip: keys = lipsync bytes (without the 0xfe terminator), mapped linearly over the clip;
    `peak` = recent audio peak (abs 16-bit sample) - below `gate` the mouth is closed (5)."""
    if t < 0 or t >= duration or not keys: return 5
    if peak is not None and peak < gate: return 5
    return keys[min(len(keys) - 1, int(t / duration * len(keys)))]
