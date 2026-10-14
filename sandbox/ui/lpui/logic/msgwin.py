"""The message window: scene dialogue, the navigator's guide lines, confirmations (TextLayout object, static; one instance
at 0x2364094 in the captures). Written from TextLayout_BuildGlyphs 0x208afa8, TextCursor_Initialize 0x208ba18 /
_InitializeSecondary 0x208bfb8, Text_StepMessageDrawProgress 0x1ffb08c / TextLayout_Update 0x208b544, TextControl_BuildText
0x208bd84 and TextControl_ProcessText 0x208bf4c.

Splitting: a message becomes up to 6 lines of at most 13 full-width cells (12 px; ASCII 6 px). A line ends at '\\n' or when
full; when full, if the next character is '\\n' it is consumed, and if it is one of the kinsoku characters (ASCII ! , . ?
or text resource 0/1/7 entries 0..6: 。、！？「」【) the line's last character moves to the next line with it.

Window: three line boxes show lines offset..offset+2 (pen x / map left per box: (11,5) (11,3) (11,1), or (11,5) (11,3)
(12,0) for messages under three lines); colour 1 (2 in the navigator's window). Typing: each frame the timer (12.12) drops
by 1.0 while >= 1; when it is below 1 a character is drawn on the first unfinished line's box and the timer reloads to
SPEED[speed] (10 / 8 / 4 / 2 / 1 frames). When the third box finishes and lines remain, the window scrolls one line: the
boxes are re-set to lines offset+1.., the first two redrawn whole, the third typed afresh."""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(HERE))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))

LINE_CELLS = 13
MAX_LINES = 6
sys.path.insert(0, os.path.dirname(os.path.dirname(HERE)))
from lpui import gamedata as _G
LAYOUT3 = _G.table(0x212750c, '2i', 3)          # (pen x, map left) per box, messages of 3+ lines
LAYOUT2 = _G.table(0x21274f4, '2i', 3)          # fewer than 3 lines
SPEED = _G.table(0x21274e0, 'i', 5)             # frames per character (12.12) by text speed
_KINSOKU = None


def kinsoku():
    global _KINSOKU
    if _KINSOKU is None:
        import txrc
        g = txrc.parse(open(os.path.join(ROOT, 'content/assets/text/gd_textres_jpn.txrc'), 'rb').read(), decrypted=True)
        _KINSOKU = [t for t in g[1][7][:7]]
    return _KINSOKU


def _char_len(b, p):
    if p >= len(b) or b[p] == 0:
        return 0
    if b[p] < 0x80:
        return 1
    n = 1
    while p + n < len(b) and (b[p + n] & 0xc0) == 0x80:
        n += 1
    return n


def _after_full(b, p, last_len):
    """TextCursor_InitializeSecondary: (byte adjustment, newline consumed)"""
    n = _char_len(b, p)
    if n == 0:
        return 0, False
    if n == 1:
        c = b[p:p + 1]
        if c == b'\n':
            return 1, True
        if c in (b'!', b',', b'.', b'?'):
            return -last_len, False
        return 0, False
    if n == 3 and any(b[p:p + 3] == k[:3] for k in kinsoku()):
        return -last_len, False
    return 0, False


def _take_line(b, p):
    """TextCursor_Initialize: (line bytes, bytes consumed)"""
    width, consumed, last = 0, 0, 0
    q = p
    newline = False
    line_len = 0
    while True:
        n = _char_len(b, q)
        line_len = consumed
        if n == 0 or width >= LINE_CELLS * 12:
            break
        width += 12 if n >= 2 else 6
        last = 3 if n >= 2 else 1
        consumed += n
        if b[q:q + 1] == b'\n':
            newline = True
            line_len = consumed - n
            q += n
            break
        q += n
    extra = 0
    if not newline:
        extra, newline = _after_full(b, q, last)
        if not newline:
            line_len += extra
    return b[p:p + line_len], consumed + extra


def split(message):
    """the window's lines (TextLayout_BuildGlyphs)"""
    b = message.encode('utf8')
    lines, p = [], 0
    for _ in range(MAX_LINES):
        line, n = _take_line(b, p)
        p += n
        if n:
            lines.append(line.decode('utf8', 'replace'))
    return lines


class MessageWindow:
    def __init__(self, message, speed=2, navi=False, name=''):
        self.lines = split(message)
        self.name = name
        self.colour = 2 if navi else 1
        self.layout = LAYOUT3 if len(self.lines) >= 3 else LAYOUT2
        self.offset = 0
        self.done = [False] * len(self.lines)
        self.typed = [0] * 3                 # characters drawn per box
        self.speed = speed
        self.timer = 0

    def current(self):
        for i, d in enumerate(self.done):
            if not d:
                return i
        return -1

    def finished(self):
        return self.current() == -1

    def boxes(self):
        """[(string, characters drawn, pen x, map left)] of the three line boxes"""
        out = []
        for i in range(3):
            j = self.offset + i
            s = self.lines[j] if j < len(self.lines) else ''
            x0, left = self.layout[i]
            out.append((s, self.typed[i], x0, left))
        return out

    def step_char(self):
        """draw the next character (Text_DrawNextChar on the current line's box)"""
        cur = self.current()
        if cur < 0:
            return
        bi = cur - self.offset
        s = self.lines[cur]
        if self.typed[bi] < len(s):
            self.typed[bi] += 1
        if self.typed[bi] >= len(s):
            self.done[cur] = True
            if not self.finished() and bi == 2:
                self.offset += 1                      # scroll: the first two boxes redrawn whole, the third typed afresh
                self.typed = [len(self.lines[self.offset]), len(self.lines[self.offset + 1]), 0]

    def tick(self):
        """one frame (TextControl_ProcessText then a character when the timer runs out)"""
        if self.finished():
            return
        if self.timer >> 12 > 0:
            self.timer -= 0x1000
        if self.timer >> 12 < 1:
            self.timer = SPEED[self.speed]
            self.step_char()

    def complete(self):
        """the player taps while typing: everything shown at once"""
        while not self.finished():
            self.step_char()
