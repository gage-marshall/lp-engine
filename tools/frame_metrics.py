#!/usr/bin/env python3
"""Face blob (skin colour) position and size in a 192x256 / 384x512 portrait frame: centroid x/y and bbox height as fractions of the frame, text window excluded.
    from frame_metrics import face_metrics; face_metrics(PIL image, text_top=0.72)"""
import numpy as np
from PIL import Image
from scipy import ndimage


def face_metrics(im, text_top=0.72):
    a = np.asarray(im.convert('RGB')).astype(np.int32)
    H, W = a.shape[:2]
    a = a[:int(H * text_top)]
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    skin = (r > 170) & (g > 110) & (b > 80) & (r > g + 12) & (g > b + 5) & (r - b > 35) & (r - b < 120)
    lab, n = ndimage.label(skin)
    if n == 0: return None
    sizes = ndimage.sum(skin, lab, range(1, n + 1))
    k = int(np.argmax(sizes)) + 1
    ys, xs = np.nonzero(lab == k)
    if len(ys) < 200: return None
    return {'cx': float(xs.mean() / W), 'cy': float(ys.mean() / H), 'h': float((ys.max() - ys.min()) / H), 'w': float((xs.max() - xs.min()) / W), 'area': float(len(ys) / (W * H))}
