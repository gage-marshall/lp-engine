"""The game's touch areas.

`HitList_Add` 0x20839a4 appends {x, y, halfW, halfH, id, enabled} (6 words, 0x18 bytes) to a list (count at +0xc04);
`HitList_Bind` puts a list in slot 0..4 of a control object; `TouchCtl_PollPress` 0x2083838 tests the touch point against
slots 0..4 in order with `Input_UpdateHeldState` 0x2083ccc: the first enabled item with x-halfW <= tx <= x+halfW and
y-halfH <= ty <= y+halfH (edges included) wins -> (slot, id). Coordinates are the touch screen's (x 0..255, y 0..191)."""


class TouchCtl:
    def __init__(self, lists):
        """lists: up to 5 entries, each None or [[x, y, halfW, halfH, id, enabled], ...]"""
        self.lists = list(lists) + [None] * (5 - len(lists))

    def hit(self, tx, ty):
        for slot, items in enumerate(self.lists[:5]):
            for x, y, hw, hh, ident, enabled in items or []:
                if enabled and x - hw <= tx <= x + hw and y - hh <= ty <= y + hh:
                    return slot, ident
        return None

    def areas(self, enabled_only=True):
        """[(slot, id, (x0, y0, x1, y1))] for drawing / crawling"""
        out = []
        for slot, items in enumerate(self.lists[:5]):
            for x, y, hw, hh, ident, enabled in items or []:
                if enabled or not enabled_only:
                    out.append((slot, ident, (x - hw, y - hh, x + hw, y + hh)))
        return out
