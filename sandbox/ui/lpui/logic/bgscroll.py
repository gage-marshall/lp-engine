"""Animated background scroll (TransitionBg, static 0x20d6130..0x20d622c; state at *0x2156b84).

LIVE pattern (top screen, sub BG1): every frame a sub-counter (+5) counts to 3, then resets and the scroll counter (+8) goes
up by one, wrapping 0x100 -> 0; sub BG1 HOFS = VOFS = counter (0x20d61ac). TransitionBg_ResetState sets the counter to 0.
Title variant (0x20d61c0): the sub-counter counts to 8 and the counter goes DOWN by one (wrap -0x100 -> 0); main BG1 VOFS
= counter, sub BG0 VOFS = counter + 0xc0."""


class LiveScroll:
    def __init__(self, counter=0, sub=0):
        self.counter, self.sub = counter, sub

    def reset(self):
        self.counter = 0

    def tick(self):
        self.sub += 1
        if self.sub == 3:
            self.sub = 0
            self.counter += 1
        if self.counter == 0x100:
            self.counter = 0
        return self.scroll()

    def scroll(self):
        """(HOFS, VOFS) of sub BG1"""
        return self.counter & 0x1ff, self.counter & 0x1ff


class TitleScroll:
    def __init__(self, counter=0, sub=0):
        self.counter, self.sub = counter, sub

    def tick(self):
        self.sub += 1
        if self.sub == 8:
            self.sub = 0
            self.counter -= 1
        if self.counter == -0x100:
            self.counter = 0
        return self.scroll()

    def scroll(self):
        """{(engine, bg): (HOFS, VOFS)}: main BG1 and sub BG0 vertical offsets"""
        return {('main', 1): (0, self.counter & 0x1ff), ('sub', 0): (0, (self.counter + 0xc0) & 0x1ff)}
