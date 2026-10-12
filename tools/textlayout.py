"""Reference implementation of the game's message text engine (layout, pagination, typewriter timing, rendering).

Rules decoded from the game code (Text_GetGlyphWidth/Text_DrawString/Text_DrawNextChar/TextBox_Layout/TextControl_*):
  * font modes: 1 = LC12 font, UTF-8 strings, advance 6 px for ASCII (< 0x80) and 12 px for everything else (default);
                2 = LC10 font, UTF-8, advance 5 / 10;  0 = LC12 font with Shift-JIS strings (ASCII mapped to full-width, advance 12).
    Only 1- to 3-byte UTF-8 sequences are supported. The font files themselves are monospaced (half = 6/5, full = 12/10), so the glyph widths agree.
  * '\\n' inside one canvas (Text_DrawString): line pitch 12 px, every following line starts 6 px to the left of the first. The message window instead uses one
    canvas per line (2 tiles = 16 px high, at BG tile x = 5, 3, 1, y = 1; canvas 2 x 20 tiles), so there the pitch is 16 px with no indent.
  * '\\\\' (backslash) draws nothing and toggles the text colour between the box colour and palette entry 3 (red highlight).
  * Greek alpha/beta/gamma/delta in the LC12 font are special glyphs: '!?', '!!', opening and closing quotation marks.
  * Each text line is a canvas of 13 full-width characters (156 px); longer lines are broken automatically (wrap(), with kinsoku). The speaker name tag canvas is
    4 full-width characters (48 px). Message boxes show 3 lines. A message is split at '\\n' into lines (64-byte slots in the game: at most 63 UTF-8 bytes per line). When a message has more
    than 3 lines the box scrolls up one line per advance; the top line index then moves by one.
  * Typewriter: one character per drawing step (2 in the fast mode); steps happen when an accumulator (12.12 fixed point) drops to <= 0, which is reloaded with
    interval[speed] / k (k = frame-time scaling); intervals in updates per character for speed settings 0..4 = 10, 8, 4, 2, 1; speed -1 = instant.
  * The text canvas is rotated 90 degrees (fonts are stored R90): the console is played held sideways ("book style"), so in the canvas text runs along y and
    lines advance along -x. Everything here is in logical (reading) orientation.
"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nftr

SPEED_INTERVAL = (10, 8, 4, 2, 1)
PALETTE = [(0, 0, 0), (248, 248, 248), (0, 0, 0), (248, 0, 0), (0, 248, 0), (0, 0, 248), (0, 248, 248), (248, 0, 248), (248, 248, 0), (216, 216, 216),
           (184, 184, 184), (120, 120, 120), (80, 80, 80), (248, 144, 72), (128, 216, 56), (88, 160, 248)]       # text palette (RGB555 table in the game image)
LINE_PITCH = 12                                         # newline inside ONE canvas (Text_DrawString): 12 px, later lines 6 px to the left
MESSAGE_LINE_PITCH = 16                                 # message window: every line is its own 2-tile-high canvas (boxes at BG tiles x = 5, 3, 1): 16 px pitch, no indent
MAX_LINE_BYTES = 63


def advance(ch, mode=1):
    """horizontal advance of one character in pixels (0 for the highlight toggle)"""
    if ch == '\\': return 0
    if mode == 0: return 12
    half, full = (6, 12) if mode == 1 else (5, 10)
    return half if ord(ch) < 0x80 else full


BOX_PX = 156                                            # message text canvas: 13 full-width characters per line
NAME_TAG_PX = 48                                        # speaker name canvas: 4 full-width characters
KINSOKU_ASCII = set("!',.?")                            # ASCII characters that may not start a line (the previous character is moved down with them)
KINSOKU_JP = ('\u3002', '\u3001', '\uff01', '\uff1f', '\u300c', '\u300d', '\u3010')   # text resource group 1 entry 7 items 0-6: 。、！？「」【


def wrap(text, max_px=BOX_PX, mode=1):
    """the game's automatic line breaking (TextCursor/TextLayout line builders): characters are added to a line while its width so far is below max_px
    (so a wide character after an odd number of narrow ones may overshoot by 6 px); an explicit newline ends the line (a newline directly after a full line is
    swallowed); if the next character may not start a line (kinsoku) the last character of the line moves to the next line. -> list of lines"""
    out = []
    for para in text.split('\n'):
        i = 0; n = len(para)
        if n == 0: out.append(''); continue
        while i < n:
            j = i; w = 0
            while j < n and w < max_px: w += advance(para[j], mode); j += 1
            if j < n and j - 1 > i and (para[j] in KINSOKU_ASCII or para[j] in KINSOKU_JP): j -= 1
            out.append(para[i:j]); i = j
    return out


def split_lines(text):
    return text.split('\n')


def check_message(text):
    """-> list of problems for a message (useful for translators): lines longer than the 64-byte slot, 4-byte UTF-8, width over the box"""
    out = []
    for i, ln in enumerate(split_lines(text)):
        if len(ln.encode('utf-8')) > MAX_LINE_BYTES: out.append('line %d is %d bytes (max %d)' % (i, len(ln.encode('utf-8')), MAX_LINE_BYTES))
        if any(ord(c) > 0xffff for c in ln): out.append('line %d has a character outside the 3-byte UTF-8 range' % i)
    return out


def layout(text, mode=1, origin=(0, 0)):
    """-> list of (line_index, char, x, y, highlighted) for every drawn character; x,y = top-left of the glyph cell"""
    out = []; hl = False
    for li, ln in enumerate(split_lines(text)):
        x = origin[0] + (0 if li == 0 else -6); y = origin[1] + li * LINE_PITCH
        for ch in ln:
            a = advance(ch, mode)
            if ch == '\\': hl = not hl; continue
            out.append((li, ch, x, y, hl)); x += a
    return out


def page_states(text, window=3):
    """visible line index ranges as the player advances: [(top, bottom_exclusive), ...]"""
    n = len(split_lines(text))
    return [(0, min(window, n))] + [(t, t + window) for t in range(1, n - window + 1)]


def typewriter_times(text, speed=3, mode=1, updates_per_second=30.0, fast=False):
    """seconds at which each drawn character appears (relative to the start of the message)"""
    iv = SPEED_INTERVAL[speed] if speed >= 0 else 0
    per = 2 if fast else 1; t = 0.0; times = []; k = 0
    for li, ch, x, y, hl in layout(text, mode):
        if k % per == 0 and k: t += iv / updates_per_second
        times.append(t); k += 1
    return times


def render(text, fonts, mode=1, box_color=2, size=None, scale=2, bg=(236, 236, 236), top=0, window=3, message_window=True):
    """render the visible window (`window` lines starting at line `top`) with the real glyph bitmaps -> PIL image"""
    from PIL import Image
    f = fonts[12 if mode in (0, 1) else 10]; pitch = MESSAGE_LINE_PITCH if message_window else LINE_PITCH
    lines = (wrap(text, BOX_PX, mode) if message_window else split_lines(text))[top:top + window]
    W = size[0] if size else 12 * 14 + 8; H = size[1] if size else pitch * window + 4
    img = Image.new('RGB', (W, H), bg); px = img.load(); hl = False
    for li, ln in enumerate(lines):
        x = 4 + (0 if (message_window or li + top == 0) else -6); y = 2 + li * pitch
        for ch in ln:
            a = advance(ch, mode)
            if ch == '\\': hl = not hl; continue
            gi = f.glyph_index(ch); w, h, rows = f.glyph_bitmap(gi); col = PALETTE[3 if hl else box_color]; left = f.metrics(gi)[0]
            for yy in range(h):
                for xx in range(w):
                    if rows[yy][xx] and 0 <= x + left + xx < W and 0 <= y + yy < H: px[x + left + xx, y + yy] = col
            x += a
    return img.resize((W * scale, H * scale), Image.NEAREST) if scale != 1 else img


def load_fonts(font_dir):
    return {12: nftr.Font(open(os.path.join(font_dir, 'LC12_UTF8_R90.NFTR'), 'rb').read()), 10: nftr.Font(open(os.path.join(font_dir, 'LC10_UTF8_R90.NFTR'), 'rb').read())}


def extract_rules(arm9):
    """text-engine constants read from the ARM9 image (palette 0x2126e58, speed intervals 0x21274e0) + the decoded rules -> dict for content/engine/text_rules.json"""
    import struct
    B = 0x2000000
    pal = struct.unpack_from('<16H', arm9, 0x2126e58 - B); iv = [v >> 12 for v in struct.unpack_from('<5I', arm9, 0x21274e0 - B)]
    return {'font_modes': {'0': {'font': 'LC12', 'strings': 'Shift-JIS', 'advance': [12, 12]}, '1': {'font': 'LC12', 'strings': 'UTF-8', 'advance': [6, 12], 'default': True},
                           '2': {'font': 'LC10', 'strings': 'UTF-8', 'advance': [5, 10]}},
            'line_pitch': LINE_PITCH, 'following_line_indent': -6, 'window_lines': 3, 'max_line_bytes': MAX_LINE_BYTES, 'escape_char': '\\', 'escape_color_index': 3,
            'text_palette_rgb555': list(pal), 'speed_updates_per_char': iv, 'fast_chars_per_step': 2,
            'special_glyphs': {'LC12': {'\u03b1': '!?', '\u03b2': '!!', '\u03b3': 'opening quotation mark', '\u03b4': 'closing quotation mark'}},
            'notes': 'ASCII advance is half of the full-width advance; fonts are monospaced; text canvas is rotated 90 degrees (book-style play)'}
