"""Top-screen stat meters (運動 / 知識 / 感性 / 魅力; static SprFx_* 0x20d2120.., fid/decomp/sprfx.c).

State (pointer at 0x21569d4): +0 mode (4 = growth animation running), +5 hide-gain flag, +8 animation step; per stat i a
0x58-byte record at +0x0c + 0x58 i... fields used: +0x22 previous value, +0x23 current value (0..255), +0x44 the base
segment's 6 sprite objects, +0x5c the change segment's 6 sprites.
Length in px = value * 176 / 192 (SprFx_UpdateData_d2344: * 0xb0 * 0x2aaaaaab >> 37), clamped to 0..176.
Base segment = the smaller of the two lengths from y 8: under 32 px as 8-px pieces (SprFx_DrawGraphicsData_d2180):
piece k at y 8 + 8 k showing sequence 0xf + size - 1 (size 8, the last the remainder; exactly 32 -> one piece 0xf + 8);
32 px or more as 32-px pieces of sequence 0x17 (d2284: piece k at y 8 + 32 k, the last pulled back to end at the length).
Change segment = |current - previous| from the end of the base: sequence base 0x21 for a gain, 0x18 for a loss
(+8 in the 32-px form), hidden when +5 is set. Other sprites of the meter stay hidden."""
SCALE_NUM, SCALE_DEN, MAX_PX = 176, 192, 176


def length(value):
    return max(0, min(MAX_PX, value * SCALE_NUM // SCALE_DEN))


def segment(y, length_px, base_small, base_big):
    """[(sequence, y)] of the pieces of one segment"""
    out = []
    if length_px <= 0:
        return out
    if length_px < 32:
        n = (length_px + 7) // 8
        rem = length_px
        for k in range(n):
            size = rem if k == n - 1 else 8
            out.append((base_small + size - 1, y + 8 * k))
            rem -= 8
        return out
    n = (length_px + 31) // 32
    for k in range(n):
        yy = y + 32 * k
        if k == n - 1:
            yy -= n * 32 - length_px
        out.append((base_big, yy))
    return out


def meter(previous, current, hide_change=False):
    """(base pieces, change pieces) for a stat whose value went previous -> current"""
    a, b = length(previous), length(current)
    lo = min(a, b)
    base = segment(8, lo, 0xf, 0x17)
    if hide_change or a == b:
        return base, []
    if b > a:                                   # gain
        change = segment(a + 8, b - a, 0x21, 0x21 + 8)
    else:                                       # loss
        change = segment(b + 8, a - b, 0x18, 0x18 + 8)
    return base, change
