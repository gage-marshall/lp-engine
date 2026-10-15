#!/usr/bin/env python3
"""Script translation: the user's own scripts as messages to translate, and the patch that puts translations in.

A message is what one show-message instruction (op 83 / 84) displays: three text buffers built by op 80 (clear),
81 (append a string of the script's string table) and 82 (append string variable N, written {vN}): 0 the message,
1 a sub label, 2 the speaker's name. A translation is committed as tables that never contain the original text:

  translation/<lang>/scripts/NNNN.json  {"messages": [{"pc": 1234, "sha": "0123456789ab", "en": "..."}]}
      per script (tmx_event.arc entry NNNN); pc = the show-message instruction, sha = hash of the original message
      with its {vN} placeholders; en = the English, no line breaks needed; its {vN} fill the original's variable
      slots in order (fewer is fine: a slot left over becomes a no-op, op 0 with the same length)
  translation/<lang>/names.json         {"sha": "English", ...}: speaker names / sub labels, by hash of the original

`apply` rebuilds tmx_event.arc into the patch overlay (patch/fs/arc/tmx_event.arc, git-ignored, read by the
engine). Each translated message is wrapped at word boundaries to the box width in pixels as the engine draws it
(patch/text.json turns on its proportional text; a variable counts VAR_PX), split at its placeholders and the pieces appended to the script's string table (its last section);
where the message's appends sit back to back (nearly always) they are written anew in the English's order
(text, variable, text...) and the space left becomes one no-op (op 0); when they need more room than the original
had, the place gets a jump (op 10) to them, written after the script, and a jump back; otherwise the op-81 instructions are pointed
at the pieces (the first fragment between two variables gets the text, any further ones an empty string). A message whose original no longer hashes the same is left alone and reported.
The script is re-encrypted with its own key (the self-keyed SBIN cipher, tools/lp_decrypt.py) and stored plain.

    lp_translate.py export CONTENT OUT_DIR       messages per script, for translating (OUT_DIR under work/)
    lp_translate.py apply CONTENT TABLE_DIR      write patch/fs/arc/tmx_event.arc beside CONTENT
    lp_translate.py check MSG_DIR TABLE...       check translation tables against the exported messages

Strings are length-prefixed UTF-8: one byte, or 80h|x then y for 80h + y bytes (Script_ReadLengthPrefixedString
0x204db4c masks the rest away), so at most 383 bytes.
"""
import hashlib
import json
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(__file__))
from lp_decrypt import decrypt  # noqa: E402
from sbin_scan import VALID  # noqa: E402

ARC_MAGIC = 0x15618
LINE_PX = 156  # the message canvas (13 full-width characters)
NAME_PX = 48  # the speaker's name tag (4 full-width characters; the tag's chevron follows it)
VAR_PX = 48  # pixels counted for a {vN} when wrapping (names)
PROPORTIONAL = True  # the engine draws one-byte characters proportionally (patch/text.json, natives::text_get_glyph_width)


class Metrics:
    """pen advance per character as the engine draws it with proportional text: a one-byte character advances to
    one pixel past its ink, against where the next character's ink starts (LC12: content/assets/test_font)"""
    def __init__(self, content):
        from nftr import Font
        f = Font(open(os.path.join(content, 'assets/test_font/LC12_UTF8_R90.NFTR'), 'rb').read())
        self.ink = {}
        for c in range(0x20, 0x7f):
            gi = f.glyph_index(chr(c))
            left = f.metrics(gi)[0]
            w, _, rows = f.glyph_bitmap(gi)
            cols = [x for x in range(w) if any(r[x] for r in rows)]
            self.ink[chr(c)] = (left + cols[0], left + cols[-1]) if cols and c != 0x20 else (0, 2)

    def width(self, text):
        px = 0
        for k, ch in enumerate(text):
            if ch in self.ink and PROPORTIONAL:
                nxt = text[k + 1] if k + 1 < len(text) else ''
                px += max(1, self.ink[ch][1] + 2 - (self.ink[nxt][0] if nxt in self.ink else 0))
            else:
                px += 6 if ord(ch) < 0x80 else 12
        return px


WORDLESS = re.compile(r'[\s\u3000\u30fb\u2026\u2025\uff01\uff1f\u3002\u3001\u301c\uff5e\u30fc!?.,~\-\u03b1-\u03b4\u25bc\u266a()=\uff08\uff09]*')


def wordless(text):
    """a message of punctuation, dots and spacing only: it stays as the original writes it"""
    return WORDLESS.fullmatch(re.sub(r'\{v\d+\}', '', text)) is not None and '{v' not in text


def sha(text):
    return hashlib.sha1(text.encode('utf-8')).hexdigest()[:12]


def arc_entries(d):
    """(offset, stored size, unpacked size or 0) per entry of a 0x15618 archive with 12-byte records"""
    assert struct.unpack_from('<I', d)[0] == ARC_MAGIC
    count = struct.unpack_from('<H', d, 4)[0]
    first = struct.unpack_from('<I', d, 12)[0]
    assert (first - 12) // count == 12, 'archive with names: not a script archive'
    return [struct.unpack_from('<3I', d, 12 + 12 * i) for i in range(count)]


def arc_build(header, blobs):
    """an archive from (bytes, unpacked size or 0) per entry, 4-aligned, in order"""
    out = bytearray(header[:12]) + bytearray(12 * len(blobs))
    out += bytes((-len(out)) & 3)
    for i, (b, u) in enumerate(blobs):
        struct.pack_into('<3I', out, 12 + 12 * i, len(out), len(b), u)
        out += b
        out += bytes((-len(out)) & 3)
    return bytes(out)


class Sbin:
    def __init__(self, raw):
        self.d = bytearray(decrypt(raw))
        assert self.d[:4] == b'SBIN', 'not a script'
        self.hdr = struct.unpack('>11I', self.d[8:0x34])
        self.entries = list(struct.unpack('>%dI' % self.hdr[0], self.d[0x40:0x40 + 4 * self.hdr[0]])) if self.hdr[0] else []
        self.str_base = self.hdr[1]

    def string_at(self, off):
        p = self.str_base + off
        n = self.d[p]
        q = p + 1
        if n & 0x80:
            n = self.d[p + 1] + 0x80
            q = p + 2
        return bytes(self.d[q:q + n]).decode('utf-8', 'replace')

    def messages(self):
        """per show-message instruction: (entry, pc, [buffer 0, 1, 2]) with each buffer a list of
        ('s', append pc, string offset, operand) / ('v', var)"""
        if not self.entries:
            return []
        ents = sorted(set(self.entries))
        out = []
        for ei, (start, end) in enumerate(zip(ents, ents[1:] + [self.str_base])):
            pc = start
            bufs = [[], [], []]
            while pc < end:
                op, n = self.d[pc], self.d[pc + 1]
                if op not in VALID or pc + 2 + 2 * n > end:
                    break
                a = struct.unpack_from('>%dH' % n, self.d, pc + 2)
                if op == 0x80 and a and a[0] < 3:
                    bufs[a[0]] = []
                elif op == 0x81 and n >= 3 and a[0] < 3:
                    v = a[1] << 16 | a[2]
                    bufs[a[0]].append(('s', pc, v & 0x7fffffff, v))
                elif op == 0x82 and n >= 2 and a[0] < 3:
                    bufs[a[0]].append(('v', a[1], pc))
                elif op in (0x83, 0x84):
                    out.append((ei, pc, [list(b) for b in bufs]))
                pc += 2 + 2 * n
        return out

    def text(self, buf):
        return ''.join(self.string_at(x[2]) if x[0] == 's' else '{v%d}' % x[1] for x in buf)

    def encrypted(self):
        # the cipher is an XOR keystream from the file's own key: applying it again encrypts
        return decrypt(bytes(self.d))


def choices(sc, pc):
    """the number of options when the show-message instruction at pc is a choice menu (op 84, mode 2..5), else 0"""
    if sc.d[pc] == 0x84 and sc.d[pc + 1] >= 1:
        n = struct.unpack_from('>H', sc.d, pc + 2)[0]
        return n if 2 <= n <= 5 else 0
    return 0


def scripts(content):
    arc = open(os.path.join(content, 'fs/arc/tmx_event.arc'), 'rb').read()
    for i, (o, s, u) in enumerate(arc_entries(arc)):
        blob = arc[o:o + s]
        if not u and blob[:4] == b'SBIN':
            yield i, Sbin(blob)


def export(content, out_dir):
    """per script: its messages in order (speaker, sub label, text with {vN}), the key of each, and the names"""
    os.makedirs(out_dir, exist_ok=True)
    names = {}
    nmsg = 0
    for i, sc in scripts(content):
        rows = []
        for ei, pc, bufs in sc.messages():
            t = sc.text(bufs[0])
            row = {'entry': ei, 'pc': pc, 'sha': sha(t), 'speaker': sc.text(bufs[2]), 'text': t}
            if bufs[1]:
                row['sub'] = sc.text(bufs[1])
            n = choices(sc, pc)
            if n:
                row['choices'] = n
            for b in (1, 2):
                for x in bufs[b]:
                    if x[0] == 's':
                        n = sc.string_at(x[2])
                        names[sha(n)] = n
            rows.append(row)
        if rows:
            json.dump({'file': i, 'messages': rows}, open(os.path.join(out_dir, '%04d.json' % i), 'w'), ensure_ascii=False, indent=0)
            nmsg += len(rows)
    json.dump(names, open(os.path.join(out_dir, 'names.json'), 'w'), ensure_ascii=False, indent=1)
    print(nmsg, 'messages,', len(names), 'names / labels ->', out_dir)


def wrap(text, metrics, width=LINE_PX):
    """word wrap at `width` pixels (a {vN} counts VAR_PX); the translator's own newlines are kept"""
    def px(s):
        return sum(VAR_PX if p.startswith('{v') else metrics.width(p) for p in re.split(r'(\{v\d+\})', s) if p)
    out = []
    for para in text.split('\n'):
        line = ''
        for word in para.split(' '):
            cand = line + ' ' + word if line else word
            if line and px(cand) > width:
                out.append(line)
                line = word
            else:
                line = cand
        out.append(line)
    return '\n'.join(out)


def split_at_vars(en):
    """the English's text pieces and, between them, its {vN} in order"""
    parts = re.split(r'\{v(\d+)\}', en)
    return parts[0::2], [int(v) for v in parts[1::2]]


def encode_string(text):
    b = text.encode('utf-8')
    if len(b) < 0x80:
        return bytes([len(b)]) + b
    if len(b) > 0x17f:
        raise ValueError('string over 383 bytes: %r' % text[:40])
    return bytes([0x80, len(b) - 0x80]) + b


def apply(content, table_dir):
    arc = open(os.path.join(content, 'fs/arc/tmx_event.arc'), 'rb').read()
    ents = arc_entries(arc)
    blobs = [(arc[o:o + s], u) for o, s, u in ents]
    names = {}
    npath = os.path.join(os.path.dirname(os.path.abspath(table_dir.rstrip('/'))), 'names.json')
    if os.path.exists(npath):
        names = json.load(open(npath))
    metrics = Metrics(content)
    done = stale = bad = files = 0
    for name in sorted(os.listdir(table_dir)):
        if not re.fullmatch(r'\d{4}\.json', name):
            continue
        i = int(name[:4])
        table = {m['pc']: m for m in json.load(open(os.path.join(table_dir, name))).get('messages', [])}
        blob, u = blobs[i]
        if u or blob[:4] != b'SBIN':
            print(name, ': not a plain script, skipped')
            continue
        sc = Sbin(blob)
        pool, index = bytearray(), {}

        def put(text):
            # an English string in the pool appended after the script (offset within the pool)
            if text not in index:
                index[text] = len(pool)
                pool.extend(encode_string(text))
            return index[text]

        repoint = []  # (append pc, operand, pool offset)
        retarget = []  # (var append pc, variable)
        nops = []  # var append pcs dropped
        rewrites = []  # (pc, bytes available, [('s', pool offset, flag) / ('v', var)]): appends written anew
        for ei, pc, bufs in sc.messages():
            for b in (1, 2):  # names and labels, by hash
                for x in bufs[b]:
                    if x[0] == 's':
                        en = names.get(sha(sc.string_at(x[2])))
                        if en is not None:
                            repoint.append((x[1], x[3], put(en)))
            m = table.get(pc)
            if not m or wordless(sc.text(bufs[0])):
                continue
            if sha(sc.text(bufs[0])) != m['sha']:
                stale += 1
                print('%s: message at %#x differs from the one translated, left as it is' % (name, pc))
                continue
            # a choice menu keeps its lines (one an option); other messages are wrapped to the box
            pieces, evars = split_at_vars(m['en'] if choices(sc, pc) else wrap(m['en'], metrics))
            slots = [x for x in bufs[0] if x[0] == 'v']
            if len(evars) > len(slots):
                bad += 1
                print('%s: message at %#x: the translation has more {vN} than the original' % (name, pc))
                continue
            ops = [(x[1] if x[0] == 's' else x[2], 8 if x[0] == 's' else 6) for x in bufs[0]]
            if ops and all(ops[j + 1][0] == ops[j][0] + ops[j][1] for j in range(len(ops) - 1)):
                # the message's appends sit back to back: written anew in the English's order
                flag = next((x[3] & 0x80000000 for x in bufs[0] if x[0] == 's'), 0)
                items = []
                for k, piece in enumerate(pieces):
                    if piece:
                        items.append(('s', put(piece), flag))
                    if k < len(evars):
                        items.append(('v', evars[k]))
                rewrites.append((ops[0][0], sum(n for _, n in ops), items))
                done += 1
                continue
            # otherwise: the fragments of each gap between variable slots get the pieces (the first of a gap the
            # text, others nothing); the slots take the English's variables, slots left over become no-ops
            gaps, cur = [], []
            for x in bufs[0]:
                if x[0] == 'v':
                    gaps.append(cur)
                    cur = []
                else:
                    cur.append(x)
            gaps.append(cur)
            pieces += [''] * (len(gaps) - len(pieces))
            if any(piece and not frags for piece, frags in zip(pieces, gaps)):
                bad += 1
                print('%s: message at %#x: text where the original has none (next to a variable)' % (name, pc))
                continue
            for piece, frags in zip(pieces, gaps):
                for k, x in enumerate(frags):
                    repoint.append((x[1], x[3], put(piece if k == 0 else '')))
            for k, x in enumerate(slots):
                (retarget.append((x[2], evars[k])) if k < len(evars) else nops.append(x[2]))
            done += 1
        # layout: the script, then detours (rewrites with no room in place), then the string pool
        size = lambda items: sum(8 if it[0] == 's' else 6 for it in items)
        sc.d += bytes((-len(sc.d)) & 3)
        det_at = len(sc.d)
        detours = []
        for at, room, items in rewrites:
            if size(items) > room:
                detours.append((at, room, items, det_at))
                det_at += size(items) + 4
        pool_base = (det_at + 3) & ~3

        def code(items):
            out = bytearray()
            for it in items:
                if it[0] == 's':
                    off = it[2] | (pool_base - sc.str_base + it[1])
                    out += bytes([0x81, 3]) + struct.pack('>3H', 0, off >> 16, off & 0xffff)
                else:
                    out += bytes([0x82, 2]) + struct.pack('>2H', 0, it[1])
            return out

        def jump(frm, to):
            rel = to - (frm + 4)
            assert -0x8000 <= rel < 0x8000, 'jump out of range'
            return bytes([0x10, 1]) + struct.pack('>h', rel)

        def nop(n):
            return bytes([0x00, (n - 2) // 2]) + bytes(n - 2) if n else b''

        tail = bytearray()
        for at, room, items, where in detours:
            block = code(items)
            tail += block + jump(where + len(block), at + room)
        sc.d += tail
        sc.d += bytes(pool_base - len(sc.d))
        sc.d += pool
        for at, room, items in rewrites:
            d = next((w for a2, _, _, w in detours if a2 == at), None)
            new = code(items) if d is None else jump(at, d)
            sc.d[at:at + room] = new + nop(room - len(new))
        for vpc, var in retarget:
            struct.pack_into('>H', sc.d, vpc + 4, var)
        for vpc in nops:
            sc.d[vpc] = 0x00
        for apc, raw, off in repoint:
            v = (raw & 0x80000000) | (pool_base - sc.str_base + off)
            struct.pack_into('>2H', sc.d, apc + 4, v >> 16, v & 0xffff)
        sc.d += bytes((-len(sc.d)) & 3)
        blobs[i] = (sc.encrypted(), 0)
        files += 1
    out = os.path.join(os.path.dirname(os.path.abspath(content)), 'patch/fs/arc/tmx_event.arc')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, 'wb').write(arc_build(arc, blobs))
    json.dump({'proportional': PROPORTIONAL}, open(os.path.join(os.path.dirname(out), '../../text.json'), 'w'))
    print('%d messages in %d scripts translated; %d stale, %d rejected -> %s' % (done, files, stale, bad, out))


def check(msg_dir, tables):
    """a translation table against the exported messages: every message keyed right, no more {vN} than the
    original, nothing over the string limit, no line breaks (a choice menu: one line an option, each fitting the
    menu), no Japanese left; prints the problems"""
    content = os.path.join(os.path.dirname(os.path.abspath(__file__)), '../content')
    metrics = Metrics(content) if os.path.isdir(content) else None
    problems = 0
    for path in tables:
        i = os.path.basename(path)[:4]
        src = {m['pc']: m for m in json.load(open(os.path.join(msg_dir, i + '.json')))['messages']}
        rows = json.load(open(path)).get('messages', [])
        seen = set()
        for r in rows:
            m = src.get(r.get('pc'))
            what = None
            if m is None:
                what = 'no message at this pc'
            elif r.get('sha') != m['sha']:
                what = 'sha does not match the message'
            elif not isinstance(r.get('en'), str):
                what = 'no "en"'
            elif len(re.findall(r'\{v\d+\}', r['en'])) > len(re.findall(r'\{v\d+\}', m['text'])):
                what = 'more {vN} than the original'
            elif set(re.findall(r'\{v\d+\}', r['en'])) - set(re.findall(r'\{v\d+\}', m['text'])):
                what = 'a {vN} the original does not have'
            elif m.get('choices') and len(r['en'].split('\n')) != m['choices']:
                what = 'a choice menu of %d options needs %d lines' % (m['choices'], m['choices'])
            elif m.get('choices') and metrics and max(metrics.width(re.sub(r'\{v\d+\}', 'X' * 8, o)) for o in r['en'].split('\n')) > LINE_PX:
                what = 'a choice option wider than the menu (%d px)' % LINE_PX
            elif not m.get('choices') and '\n' in r['en']:
                what = 'line break in "en" (the patch wraps)'
            elif re.search(r'[\u3040-\u30ff\u4e00-\u9fff]', r['en']):
                what = 'Japanese left in "en"'
            elif any(m['text'].count(c) > r['en'].count(c) for c in '\u25bc\u266a'):
                what = 'a heart (\u25bc) or note (\u266a) of the original is missing'
            elif len(r['en'].encode()) > 300:
                what = 'longer than 300 bytes'
            elif not wordless(m['text']) and not re.search(r'[A-Za-z0-9{]', r['en']):
                what = 'only punctuation for a message with words'
            if what:
                problems += 1
                print('%s pc %s: %s' % (path, r.get('pc'), what))
            seen.add(r.get('pc'))
        missing = [pc for pc, m in src.items() if pc not in seen and not wordless(m['text'])]
        if missing:
            problems += 1
            print('%s: %d messages not translated: %s' % (path, len(missing), missing[:8]))
    names = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(tables[0]))), 'names.json') if tables else ''
    if metrics and os.path.exists(names):
        for k, v in json.load(open(names)).items():
            if metrics.width(v) > NAME_PX:
                problems += 1
                print('%s: %r is wider than the name tag (%d px)' % (names, v, NAME_PX))
    print('%d problems in %d tables' % (problems, len(tables)))
    return problems


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else ''
    if cmd == 'export':
        export(sys.argv[2], sys.argv[3])
    elif cmd == 'apply':
        apply(sys.argv[2], sys.argv[3])
    elif cmd == 'check':
        sys.exit(1 if check(sys.argv[2], sys.argv[3:]) else 0)
    else:
        sys.exit(__doc__)
