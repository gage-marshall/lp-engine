"""Game function addresses by name (static code, fid/labels_all.csv), and the game's literal-pool words.
    F.Main_IncStateDepth -> 0x203dba4
Labels that turned out wrong get a corrected alias (the label stays usable): the screen manager's Sprite_* names."""
import csv, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, os.path.join(ROOT, 'sandbox/ui'))
from lpui import gamedata as G  # noqa: E402


class _Names:
    pass


F = _Names()
NAME = {}
for row in csv.reader(open(os.path.join(ROOT, 'fid/labels_all.csv'))):
    if len(row) >= 3 and row[0] == 'static' and row[1][:1].isdigit():
        a = int(row[1], 16)
        if not hasattr(F, row[2]):
            setattr(F, row[2], a)
        NAME.setdefault(a, row[2])

ALIASES = {
    'Main_SetState': 'Main_FreeStateBuffer',            # sets the state at the current depth, resets deeper levels
    'Screen_Request': 'Sprite_UpdateAnimationFrame',    # (kind, screen id, entry argument)
    'Screen_Run': 'Sprite_DispatchFrameState',          # -1 while the screen runs, then its result
    'Screen_Transition': 'Sprite_SelectContextMode',
    'Screen_LoadOverlay': 'Sprite_SelectRenderState',
    'Screen_UnloadOverlay': 'Sprite_SelectTextState',
    'Screen_RunCurrent': 'Sprite_DispatchAnimationState',
    'Screen_GetNext': 'Sprite_GetField4',
    'Screen_GetResult': 'Sprite_GetField14',
    'Screen_GetAltResult': 'Sprite_GetField1c',
    'Screen_GetSaved': 'Sprite_GetContext',             # the persistent screen block 0x2152ed8 (word 0: room entry)
    'Screen_GetSavedSize': 'Sprite_GetContextValue',
}
for new, old in ALIASES.items():
    setattr(F, new, getattr(F, old))
    NAME[getattr(F, old)] = new


def W(addr, ov=None):
    """a word of the game image (literal pools hold the machines' larger constants)"""
    return G.word(addr) if ov is None else G.word(addr, ov=ov)
