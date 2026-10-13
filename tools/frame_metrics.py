#!/usr/bin/env python3
"""Face (skin colour) position and size in a portrait 3D-screen frame: robust percentiles of the skin mask, text window excluded.  Fractions of the frame."""
import numpy as np


def face_metrics(im, text_top=0.72):
    a = np.asarray(im.convert('RGB')).astype(np.int32)
    H, W = a.shape[:2]
    a = a[:int(H * text_top)]
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    skin = (r > 170) & (g > 110) & (b > 80) & (r > g + 12) & (g > b + 5) & (r - b > 35) & (r - b < 120)
    ys, xs = np.nonzero(skin)
    if len(ys) < 300: return None
    lo, hi = lambda v: float(np.percentile(v, 3)), lambda v: float(np.percentile(v, 97))
    return {'cx': float(np.median(xs) / W), 'cy': float(np.median(ys) / H), 'h': (hi(ys) - lo(ys)) / H, 'w': (hi(xs) - lo(xs)) / W, 'area': float(len(ys) / (W * H))}
