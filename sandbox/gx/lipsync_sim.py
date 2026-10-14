"""Lip-sync driver, ported: voice PCM + lip-sync keys -> mouth calls per game update.

  VoiceQueue_UpdateMode 0x209ab74 (per update while a voice plays): peak = GameSnd_GetStreamPeak (max |sample| over every
  5th sample of the stream blocks decoded since the last update; GameSnd_StreamAccumulatePeak 0x2078088, block loop in
  GameSnd_StreamThreadMain 0x207785c); peak < 1: nothing; peak < threshold (+0xb12, 1000): shape 5, speaking flag off;
  else flag on and shape = keys[elapsed * n / duration] through VoiceQueue_PickRandomPattern 0x209ad64 (shape 5 is replaced
  by a fixed rotating pattern, table 0x21046f4); VoiceQueue_DispatchByMode -> Model_SetMotionModeFromTable(mode =
  NextModeIndex(shape), hold 20). The line ends (shape 5) when elapsed >= duration.
  elapsed (+0xafc) counts video frames while the flag is on (VBlank IRQ). duration = Siren14 frames of the clip.
  keys = lipsync/lipsync.dat at lipsync_add.dat[clip] (size - 2 bytes). PCM = tools/siren14dec.c (bit-exact decoder).
"""
import os, struct, subprocess, sys, wave
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
import scene_data as sd  # noqa: E402

PATTERN = [struct.unpack_from('<i', sd.MAIN, 0x21046f4 - 0x2000000 + 4 * i)[0] for i in range(16)]
DEC = os.path.join(ROOT, 'sandbox', 'gx', 'siren14dec')
CACHE = os.path.join(ROOT, 'sandbox', 'rec', 'out', 'voice_pcm')


def pcm(clip):
    """32 kHz PCM of a voice clip (game decoder port)"""
    os.makedirs(CACHE, exist_ok=True)
    p = os.path.join(CACHE, '%d.wav' % clip)
    if not os.path.exists(p):
        if not os.path.exists(DEC):
            subprocess.run(['clang', '-O2', '-o', DEC, os.path.join(ROOT, 'tools', 'siren14dec.c')], check=True)
        sysd = os.path.join(ROOT, 'content', 'assets', 'sys')
        subprocess.run([DEC, os.path.join(ROOT, 'content', 'engine', 'g7221.pack'), os.path.join(sysd, 'Siren14Voice.vtb'),
                        os.path.join(sysd, 'Siren14Voice.spp'), str(clip), p], check=True, capture_output=True)
    w = wave.open(p)
    return np.frombuffer(w.readframes(w.getnframes()), '<i2').astype(np.int32)


def keys(clip):
    d = os.path.join(ROOT, 'content', 'assets', 'lipsync')
    o, n = struct.unpack_from('<II', open(os.path.join(d, 'lipsync_add.dat'), 'rb').read(), 8 * clip)
    return list(open(os.path.join(d, 'lipsync.dat'), 'rb').read()[o:o + max(0, n - 2)])


def block_peaks(clip):
    x = pcm(clip); n = 640
    out = []
    for k in range(len(x) // n):
        s = x[k * n:(k + 1) * n][::5][:min(256, n // 5)]
        out.append(int(np.abs(s).max()))
    return out


THRESHOLD = 0x3e8     # VoiceQueue_Init 0x209a630: +0xb12 (speaking threshold); script cmd 0x1b3 (value, 0) changes it
REST_HOLD = 0x3c      # +0xb14 (updates of shape 5 before the rest pattern); script cmd 0x1b3 (value, 1)


class LipSync:
    def __init__(self, threshold=THRESHOLD, rest_hold=REST_HOLD):
        self.active = False; self.thr = threshold; self.b14 = rest_hold

    def play(self, clip, frame):
        self.clip = clip; self.k = keys(clip); self.peaks = block_peaks(clip); self.dur = len(self.peaks)
        self.start = frame; self.el = 0; self.flag = 0; self.b04 = 0; self.b08 = 0; self.active = True; self.done_block = -1

    def set_param(self, which, value):                # VoiceQueue_SetParam 0x209af4c via cmd 0x1b3
        if which == 0: self.thr = value
        elif which == 1: self.b14 = value

    def frames(self, f0, f1):
        """video frames f0 < f <= f1 pass: elapsed += 1 per frame while the flag is on"""
        if self.active and self.flag:
            self.el += (f1 - f0)

    PREFILL = 3                       # blocks decoded right after PlayEntry (stream ring prefill)
    RATE = 50 / 59.8261               # then real time: 50 blocks (20 ms) per second at the DS frame rate

    def decoded(self, frame):
        if frame < self.start + 1:
            return 0
        n = min(self.PREFILL, frame - self.start) + max(0, int((frame - self.start - self.PREFILL) * self.RATE))
        return min(self.dur, n)

    def update(self, frame, lead=0.0):
        """one VoiceQueue_UpdateMode at video frame `frame`; returns the dispatched shapes"""
        if not self.active:
            return []
        t = (frame - self.start) * (1000 / 59.8261)
        last = self.decoded(frame) - 1
        blocks = self.peaks[self.done_block + 1:last + 1]; self.done_block = max(self.done_block, last)
        peak = max(blocks) if blocks else 0
        self.flag = 0
        if peak < 1:
            self.b04 = self.b08 = 0
            if last >= self.dur - 1 and t >= self.dur * 20:
                self.active = False; return [5]
            return []
        if peak < self.thr:
            v = 5
        else:
            self.flag = 1
            dur = max(self.dur, 1)
            if dur < self.el: self.el = dur
            v = self.pick(peak, self.k[self.el * len(self.k) // dur] if self.k else 5)
        out = [v]
        if self.dur <= self.el:
            out.append(5); self.active = False
        return out

    def pick(self, peak, v):
        if v == 5:
            if self.b04 < self.b14:
                self.b04 += 1
            else:
                self.b04 = 0
                v = PATTERN[(peak > 0x5d) * 8 + (self.b08 // 2) % 7]
                self.b08 += 1
                if self.b08 > 999: self.b08 = 0
        else:
            self.b04 = self.b08 = 0
        return v


def next_mode(v):                                   # VoiceQueue_NextModeIndex
    return {0: 1, 1: 2, 2: 3, 3: 4, 4: 5, 5: 0}.get(v, 0)
