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
    lp_translate.py apply CONTENT TABLE_DIR      write patch/fs/arc/tmx_event.arc beside CONTENT (and the text
                                                 resources from TABLE_DIR/../text, as apply-text)
    lp_translate.py check MSG_DIR TABLE...       check translation tables against the exported messages
    lp_translate.py export-text CONTENT OUT_DIR  the TXRC text resources, per file, with the box of each group
    lp_translate.py apply-text CONTENT TABLE_DIR write patch/fs/text/*.txrc (translation/<lang>/text/<file>.json)
    lp_translate.py check-text EXPORT_DIR TABLE...  check TXRC tables: keys, variables, fit in the box

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
    """pen advance per character as the engine draws it with proportional text: a one-byte character advances its
    ink width + 1 (LC12: content/assets/test_font)"""
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
            if ch in self.ink and PROPORTIONAL:  # ink width + 1 (the patched fonts draw from the first inked column)
                px += max(1, self.ink[ch][1] + 2 - self.ink[ch][0])
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
        if u and blob[:1] == b'\x10':  # stored LZ10-compressed (about 500 scripts): unpacked; apply stores them plain
            from lp_gfx import lz10
            blob = lz10(blob)
        if blob[:4] == b'SBIN':
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
    speakers = {}  # English name -> the originals it replaces
    for name in sorted(os.listdir(table_dir)):
        if not re.fullmatch(r'\d{4}\.json', name):
            continue
        i = int(name[:4])
        table = {m['pc']: m for m in json.load(open(os.path.join(table_dir, name))).get('messages', [])}
        blob, u = blobs[i]
        if u and blob[:1] == b'\x10':  # a compressed script: unpacked (written back plain)
            from lp_gfx import lz10
            blob, u = lz10(blob), 0
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
                        orig = sc.string_at(x[2])
                        en = names.get(sha(orig))
                        if en is not None:
                            repoint.append((x[1], x[3], put(en)))
                            speakers.setdefault(en, set()).add(orig)
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
    # the game tells who speaks by comparing the name shown with its own (Japanese) list (0x20507fc, used by lip
    # sync); the engine maps each English name back to the originals it replaces (patch/text.json "speakers")
    speakers = {en: sorted(o) for en, o in sorted(speakers.items())}
    json.dump({'proportional': PROPORTIONAL, 'speakers': speakers}, open(os.path.join(os.path.dirname(out), '../../text.json'), 'w'), ensure_ascii=False)
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
        table = json.load(open(path))
        rows = table.get('messages', [])
        seen = set(table.get('keep', []))  # pcs left in Japanese on purpose (the Japanese-language quiz)
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


# ---- TXRC text resources (text/gd_textres*_jpn.txrc): menus, items, mail, help ----------------------------------

TEXT_DIR = 'fs/text'
TEXT_TOKEN = re.compile(r'\[[pkat]-name\d\]|%[0-9]*[ds]')
# (file, group, sub) -> how a group is treated; sub None = the whole group, group None = the whole file
TEXT_KEEP = {('gd_textres03_jpn', None, None): 'kana keyboards, name readings and the word filter (input data)',
             ('gd_textres_jpn', 0, None): 'name readings the voice matches the player\'s name against',
             ('gd_textres07_jpn', 0, 13): 'Japanese vocabulary quiz (undecided)'}
TEXT_PARTIAL = {('gd_textres01_jpn', 12, None), ('gd_textres01_jpn', 13, None), ('gd_textres01_jpn', 20, None)}  # credits: names stay
TEXT_WINDOW = {('gd_textres02_jpn', 1, None), ('gd_textres01_jpn', 4, None)}  # shown in the message window: LINE_PX, pages
TEXT_AUTO = {('gd_textres06_jpn', 0, 1): 216}  # wrapped by the game in a box measured on screen (px)
TEXT_FLOW = {('gd_textres07_jpn', None, None)}  # values the game fills into other text ({vN}, [x-name]): no box of their own
# a one-line group's box is only known to be as wide as its longest original; English may run to SOFT_PX of that
SOFT = lambda px: max(px * 3 // 2, px + 24)


def text_rule(table, file, g, s):
    for key in ((file, g, s), (file, g, None), (file, None, None)):
        if key in table:
            return table[key] if isinstance(table, dict) else True
    return None


def jp_px(line):
    """pixels of a line of the original as the game lays it out: full-width 12, half-width 6, a variable VAR_PX"""
    px = 0
    for part in re.split(r'(' + TEXT_TOKEN.pattern + r')', line):
        if TEXT_TOKEN.fullmatch(part or 'x'):
            px += VAR_PX
        else:
            px += sum(6 if ord(c) < 0x80 or 0xff61 <= ord(c) <= 0xff9f else 12 for c in part)
    return px


def text_box(name, g, s, texts):
    """the box a group is drawn in: width px and most lines (0: pages); `soft`: a one-line group, whose original
    only shows a lower bound (the English may run to SOFT of it, listed for a look on screen); `flow`: no box"""
    width = max([jp_px(x) for t in texts for x in t.split('\n')] + [12])
    lines = max([t.count('\n') + 1 for t in texts] + [1])
    if text_rule(TEXT_WINDOW, name, g, s):
        return {'width': LINE_PX, 'lines': 0, 'window': True}
    if text_rule(TEXT_AUTO, name, g, s):
        return {'width': text_rule(TEXT_AUTO, name, g, s), 'lines': 0, 'auto': True}
    if text_rule(TEXT_FLOW, name, g, s):
        return {'width': 10000, 'lines': 0, 'flow': True}
    if lines == 1:
        return {'width': SOFT(width), 'lines': 1, 'soft': width}
    return {'width': width, 'lines': lines}


def text_files(content):
    from txrc import parse
    d = os.path.join(content, TEXT_DIR)
    for f in sorted(os.listdir(d)):
        if f.endswith('.txrc'):
            raw = open(os.path.join(d, f), 'rb').read()
            yield f[:-5], raw, parse(raw)


def export_text(content, out_dir):
    """per TXRC file: its groups (translate / keep / partial, the box each is drawn in) and their lines"""
    os.makedirs(out_dir, exist_ok=True)
    total = 0
    for name, raw, groups in text_files(content):
        out = []
        for g, subs in enumerate(groups):
            for s, lines in enumerate(subs):
                texts = [l.decode('utf-8', 'replace').replace('\r\n', '\n') for l in lines]
                keep = text_rule(TEXT_KEEP, name, g, s)
                box = {'width': max([jp_px(x) for t in texts for x in t.split('\n')] + [12]),
                       'lines': max([t.count('\n') + 1 for t in texts] + [1])}
                if text_rule(TEXT_WINDOW, name, g, s):
                    box = {'width': LINE_PX, 'lines': 0, 'window': True}
                auto = text_rule(TEXT_AUTO, name, g, s)
                if auto:
                    box = {'width': auto, 'lines': 0, 'auto': True}
                box = text_box(name, g, s, texts)
                grp = {'id': '%d.%d' % (g, s), 'box': box}
                if keep:
                    grp['keep'] = keep
                if text_rule(TEXT_PARTIAL, name, g, s):
                    grp['partial'] = True
                grp['rows'] = [{'id': '%d.%d.%d' % (g, s, l), 'sha': sha(t), 'text': t} for l, t in enumerate(texts)
                               if t.strip() and not wordless(t)]
                if not keep:
                    total += len(grp['rows'])
                out.append(grp)
        json.dump({'file': name, 'groups': out}, open(os.path.join(out_dir, name + '.json'), 'w'), ensure_ascii=False, indent=1)
    print(total, 'lines to translate ->', out_dir)


def en_px(text, metrics):
    return sum(VAR_PX if TEXT_TOKEN.fullmatch(p) else metrics.width(p) for p in re.split(r'(' + TEXT_TOKEN.pattern + r')', text) if p)


def wrap_text(en, metrics, box):
    """the English wrapped to its box: words to the width (a variable counts VAR_PX), the translator's own line
    breaks kept; the lines that do not fit (a single word wider than the box, or more lines than the box has)"""
    out = []
    for para in en.split('\n'):
        line = ''
        for word in para.split(' '):
            cand = line + ' ' + word if line else word
            if line and en_px(cand, metrics) > box['width']:
                out.append(line)
                line = word
            else:
                line = cand
        out.append(line)
    bad = [l for l in out if en_px(l, metrics) > box['width']]
    if box['lines'] and len(out) > box['lines']:
        bad.append('%d lines, the box has %d' % (len(out), box['lines']))
    return out, bad


def check_text(export_dir, tables):
    """TXRC translation tables against the export: keys, variables (format codes in the same order), fit in the box"""
    metrics = Metrics(os.path.join(os.path.dirname(os.path.abspath(__file__)), '../content'))
    problems = 0
    for path in tables:
        name = os.path.basename(path).split('.')[0]  # <file>.json or <file>.<part>.json
        part = os.path.basename(path).count('.') > 1
        ex = json.load(open(os.path.join(export_dir, name + '.json')))
        src = {r['id']: (grp, r) for grp in ex['groups'] for r in grp['rows']}
        seen = set()
        wider = []
        for r in json.load(open(path)).get('rows', []):
            grp, m = src.get(r.get('id'), (None, None))
            what = None
            if m is None:
                what = 'no line with this id'
            elif grp.get('keep'):
                what = 'this group is kept as it is (%s)' % grp['keep']
            elif r.get('sha') != m['sha']:
                what = 'sha does not match the line'
            elif not isinstance(r.get('en'), str) or not r['en'].strip():
                what = 'no "en"'
            elif re.findall(r'%[0-9]*[ds]', r['en']) != re.findall(r'%[0-9]*[ds]', m['text']):
                what = 'format codes (%d, %s) must stay the same and in the same order'
            elif set(TEXT_TOKEN.findall(r['en'])) - set(TEXT_TOKEN.findall(m['text'])):
                what = 'a variable the original does not have'
            elif '[k-name1]' in r['en'] or '[p-name1]' in r['en']:
                what = '[k-name1] (her word for herself) / [p-name1] (his): write I / me instead'
            elif re.search(r'[぀-ヿ一-鿿]', r['en']) and not grp.get('partial'):
                what = 'Japanese left in "en"'
            elif any(m['text'].count(c) > r['en'].count(c) for c in '▼♪'):
                what = 'a heart (▼) or note (♪) of the original is missing'
            else:
                lines, bad = wrap_text(r['en'], metrics, grp['box'])
                if bad:
                    what = 'does not fit its box (%d px%s): %s' % (grp['box']['width'], ', %d lines' % grp['box']['lines'] if grp['box']['lines'] else '', bad[0])
                elif len('\r\n'.join(lines).encode()) > 1023:
                    what = 'longer than 1023 bytes'
            if what:
                problems += 1
                print('%s %s: %s' % (path, r.get('id'), what))
            elif grp['box'].get('soft') and en_px(r['en'], metrics) > grp['box']['soft']:
                wider.append(r['id'])
            seen.add(r.get('id'))
        if wider:
            print('%s: %d lines wider than the original (%s...): look at them on screen' % (path, len(wider), ', '.join(wider[:6])))
        missing = [i for i, (grp, m) in src.items() if i not in seen and not grp.get('keep') and not grp.get('partial')]
        if missing and not part:
            problems += 1
            print('%s: %d lines not translated: %s' % (path, len(missing), missing[:8]))
    print('%d problems in %d tables' % (problems, len(tables)))
    return problems


def apply_text(content, table_dir):
    """each translated TXRC rebuilt into the patch overlay (patch/fs/text/), the English wrapped to its box"""
    from txrc import build
    metrics = Metrics(content)
    out_dir = os.path.join(os.path.dirname(os.path.abspath(content)), 'patch', TEXT_DIR)
    os.makedirs(out_dir, exist_ok=True)
    done = stale = 0
    for name, raw, groups in text_files(content):
        paths = sorted(p for p in os.listdir(table_dir) if p.split('.')[0] == name and p.endswith('.json'))
        if not paths:
            continue
        boxes = {}
        for g, subs in enumerate(groups):
            for s, lines in enumerate(subs):
                texts = [l.decode('utf-8', 'replace').replace('\r\n', '\n') for l in lines]
                boxes[(g, s)] = text_box(name, g, s, texts)
        for r in (r for p in paths for r in json.load(open(os.path.join(table_dir, p))).get('rows', [])):
            g, s, l = (int(x) for x in r['id'].split('.'))
            if text_rule(TEXT_KEEP, name, g, s) or sha(groups[g][s][l].decode('utf-8', 'replace').replace('\r\n', '\n')) != r['sha']:
                stale += 1
                continue
            lines, _ = wrap_text(r['en'], metrics, boxes[(g, s)])
            # as many lines as the original: a box that takes its lines one by one would otherwise read past the end
            # (lines end in CR LF or LF alone; the original's own ending is kept)
            orig = groups[g][s][l]
            orig_lines = orig.replace(b'\r\n', b'\n').count(b'\n') + 1
            lines += [' '] * (orig_lines - len(lines))  # a space: some boxes draw an empty line as stale bytes
            groups[g][s][l] = ('\n' if b'\n' in orig and b'\r\n' not in orig else '\r\n').join(lines).encode('utf-8')
            done += 1
        open(os.path.join(out_dir, name + '.txrc'), 'wb').write(build(groups, raw))
    if PROPORTIONAL:  # the fonts' own ASCII advances proportional too: the text canvases' SDK drawing reads them
        import nftr
        fdir = os.path.join(os.path.dirname(os.path.abspath(content)), 'patch/fs/test_font')
        os.makedirs(fdir, exist_ok=True)
        for name, space in (('LC12_UTF8_R90.NFTR', 4), ('LC10_UTF8_R90.NFTR', 3)):
            data = open(os.path.join(content, 'fs/test_font', name), 'rb').read()
            open(os.path.join(fdir, name), 'wb').write(nftr.proportional(data, space))
    print('%d text lines translated; %d stale -> %s' % (done, stale, out_dir))


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else ''
    if cmd == 'export':
        export(sys.argv[2], sys.argv[3])
    elif cmd == 'apply':
        apply(sys.argv[2], sys.argv[3])
        text = os.path.join(os.path.dirname(os.path.abspath(sys.argv[3].rstrip('/'))), 'text')
        if os.path.isdir(text):
            apply_text(sys.argv[2], text)
    elif cmd == 'export-text':
        export_text(sys.argv[2], sys.argv[3])
    elif cmd == 'apply-text':
        apply_text(sys.argv[2], sys.argv[3])
    elif cmd == 'check-text':
        sys.exit(1 if check_text(sys.argv[2], sys.argv[3:]) else 0)
    elif cmd == 'check':
        sys.exit(1 if check(sys.argv[2], sys.argv[3:]) else 0)
    else:
        sys.exit(__doc__)
