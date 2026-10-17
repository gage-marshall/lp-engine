#!/usr/bin/env python3
"""The distributable translation: the patch overlay as a delta against the player's own extracted files.

The patch tree (patch/, built by lp_translate.py / lp_gfx.py from content/) holds modified copies of the game's files,
which cannot be distributed. This writes it as copy / insert operations against a reference built from the player's
original file at the same path: the file itself, then (recursively) every archive entry decompressed and every
SBIN / TXRC decrypted ("views"; the engine's applier, lp_engine::patchfile, builds the same reference). What is
copied is the player's data; what is inserted is the translation's own (English text, redrawn tiles, tables).

    lp_delta.py build CONTENT PATCH OUT.lpdelta    write the delta (prints how much it inserts, and how much of that
                                                   also occurs in the original files: should be next to nothing)
    lp_delta.py apply CONTENT OUT.lpdelta DIR      rebuild the patch tree into DIR (check: diff -r PATCH DIR)

Format (all integers little-endian): b"LPDELTA1", u32 header length, the header (UTF-8 JSON: {"version": 1,
"rom_sha1": ..., "files": [{"path", "size", "sha1", "ref": path under CONTENT or null, "ops": length}]}), then the
files' operation streams back to back, the whole after the header deflate-compressed (zlib). An operation stream is
varints: (n << 1) then the source offset: copy n bytes of the reference; (n << 1 | 1) then n bytes: insert them.
"""
import hashlib
import json
import os
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lp_decrypt import decrypt  # noqa: E402
from unpack_arc import maybe_lz  # noqa: E402

MAGIC = b'LPDELTA1'
ARC_MAGIC = 0x15618
KEY = 16  # bytes a match must at least share
STRIDE = 4  # reference positions indexed


def views(b, include_self=True):
    """the file and what is inside it: archive entries decompressed (recursively), SBIN / TXRC decrypted"""
    if include_self:
        yield b
    if len(b) > 12 and struct.unpack_from('<I', b)[0] == ARC_MAGIC:
        cnt = struct.unpack_from('<H', b, 4)[0]
        first = struct.unpack_from('<I', b, 12)[0] if cnt else 0
        stride = (first - 12) // cnt if cnt and first > 12 else 12
        for i in range(cnt):
            o, s, u = struct.unpack_from('<3I', b, 12 + i * stride)
            raw = b[o:o + s]
            e = maybe_lz(raw, u)
            yield from views(e, e is not raw)
    elif b[:4] in (b'SBIN', b'TXRC') and len(b) >= 8:
        yield decrypt(b)


def reference(content, path):
    """the reference for the patched file at `path` (relative to the patch tree: fs/... maps to CONTENT/fs/...)"""
    src = os.path.join(content, path)
    if not path.startswith('fs/') or not os.path.isfile(src):
        return None, b''
    return path, b''.join(views(open(src, 'rb').read()))


def varint(n):
    out = bytearray()
    while True:
        b = n & 0x7f
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def diff(ref, tgt):
    """copy / insert operations turning `ref` into `tgt`; (ops bytes, inserted byte count)"""
    index = {}
    for p in range(0, len(ref) - KEY + 1, STRIDE):
        index.setdefault(ref[p:p + KEY], p)
    ops = bytearray()
    lit = bytearray()
    inserted = 0
    i = 0

    def flush():
        nonlocal inserted
        if lit:
            ops.extend(varint(len(lit) << 1 | 1))
            ops.extend(lit)
            inserted += len(lit)
            lit.clear()
    while i < len(tgt):
        best = None
        # a match covering i starts at most STRIDE - 1 bytes before an indexed position
        for k in range(STRIDE):
            j = i + k
            p = index.get(tgt[j:j + KEY]) if j + KEY <= len(tgt) else None
            if p is None:
                continue
            s, r = j, p
            while s > i and r > 0 and tgt[s - 1] == ref[r - 1]:  # extend back to i
                s -= 1
                r -= 1
            if s != i:
                continue
            n = j - s + KEY
            while i + n < len(tgt) and r + n < len(ref) and tgt[i + n] == ref[r + n]:
                n += 1
            if best is None or n > best[1]:
                best = (r, n)
            break
        if best and best[1] >= KEY:
            flush()
            ops.extend(varint(best[1] << 1))
            ops.extend(varint(best[0]))
            i += best[1]
        else:
            lit.append(tgt[i])
            i += 1
    flush()
    return bytes(ops), inserted


def apply_ops(ref, ops):
    out = bytearray()
    p = 0

    def rd():
        nonlocal p
        v = s = 0
        while True:
            b = ops[p]
            p += 1
            v |= (b & 0x7f) << s
            s += 7
            if not b & 0x80:
                return v
    while p < len(ops):
        t = rd()
        n = t >> 1
        if t & 1:
            out += ops[p:p + n]
            p += n
        else:
            o = rd()
            out += ref[o:o + n]
    return bytes(out)


def build(content, patch, out):
    files = []
    streams = []
    total = ins = 0
    for root, _, fs in os.walk(patch):
        for f in sorted(fs):
            if f.startswith('.'):
                continue
            full = os.path.join(root, f)
            rel = os.path.relpath(full, patch).replace(os.sep, '/')
            tgt = open(full, 'rb').read()
            refpath, ref = reference(content, rel)
            ops, n = diff(ref, tgt)
            assert apply_ops(ref, ops) == tgt, rel
            files.append({'path': rel, 'size': len(tgt), 'sha1': hashlib.sha1(tgt).hexdigest(), 'ref': refpath, 'ops': len(ops)})
            streams.append(ops)
            total += len(tgt)
            ins += n
            print('%-40s %9d bytes, %8d inserted' % (rel, len(tgt), n))
    rom = json.load(open(os.path.join(content, 'manifest.json')))['rom']['sha1']
    header = json.dumps({'version': 1, 'rom_sha1': rom, 'files': files}).encode()
    body = zlib.compress(b''.join(streams), 9)
    open(out, 'wb').write(MAGIC + struct.pack('<I', len(header)) + header + body)
    print('%d files, %d bytes; %d inserted (%.1f%%); delta %d bytes' % (len(files), total, ins, 100 * ins / max(total, 1), os.path.getsize(out)))
    leak(content, files, streams)


def leak(content, files, streams, KEY=32):
    """how many inserted bytes also occur in the original files (32-byte windows): the translation must carry its own
    data only"""
    windows = set()
    for root, _, fs in os.walk(os.path.join(content, 'fs')):
        for f in fs:
            b = open(os.path.join(root, f), 'rb').read()
            for v in views(b):
                for p in range(0, len(v) - KEY + 1, KEY):
                    w = v[p:p + KEY]
                    if any(w):
                        windows.add(w)
    hit = checked = 0
    for meta, ops in zip(files, streams):
        p = 0
        lits = []
        while p < len(ops):
            v = s = 0
            while True:
                b = ops[p]; p += 1; v |= (b & 0x7f) << s; s += 7
                if not b & 0x80:
                    break
            n = v >> 1
            if v & 1:
                lits.append(ops[p:p + n]); p += n
            else:
                while ops[p] & 0x80:
                    p += 1
                p += 1
        for lt in lits:
            for q in range(0, len(lt) - KEY + 1):
                checked += 1
                if lt[q:q + KEY] in windows:
                    hit += 1
    print('inserted 32-byte windows found in the original files: %d of %d' % (hit, checked))


def apply(content, delta, out_dir):
    d = open(delta, 'rb').read()
    assert d[:8] == MAGIC
    hl = struct.unpack_from('<I', d, 8)[0]
    header = json.loads(d[12:12 + hl])
    body = zlib.decompress(d[12 + hl:])
    p = 0
    for f in header['files']:
        ops = body[p:p + f['ops']]
        p += f['ops']
        _, ref = reference(content, f['path']) if f['ref'] else (None, b'')
        b = apply_ops(ref, ops)
        assert hashlib.sha1(b).hexdigest() == f['sha1'], f['path']
        dst = os.path.join(out_dir, f['path'])
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        open(dst, 'wb').write(b)
    print('%d files -> %s' % (len(header['files']), out_dir))


if __name__ == '__main__':
    if len(sys.argv) >= 5 and sys.argv[1] == 'build':
        build(sys.argv[2], sys.argv[3], sys.argv[4])
    elif len(sys.argv) >= 5 and sys.argv[1] == 'apply':
        apply(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit(__doc__)
