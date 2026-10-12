#!/usr/bin/env python3
"""Record live script/dialogue state from a headless DeSmuME saved state.

This is an oracle for ``event_trace.py`` and later ports, not a ROM extractor:
it reads a user-local ROM and state at runtime and writes no game data into the
repository. The JSON contains only decoded execution observations requested by
the caller (script reference, PC, command word, and displayed strings).

Example (from the repository root):
    .venv/bin/python tools/emu_script_oracle.py content \\
      --state tools/emu_scripts/states/st5.dst --advances 8 --json /tmp/st5.json
"""
import argparse
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)


SCRIPT_CONTEXT_PTR = 0x021415B8
CURRENT_COMMAND = 0x10C0
CURRENT_PC = 0x10C8
TEXT_BUFFER = 0x10DC
SUB_BUFFER = 0x10E0
NAME_BUFFER = 0x10E4
CURRENT_REFERENCE = 0x10F8


def decode_reference(raw):
    """Decode the packed slot/script/entry reference used by ScriptContext."""
    return {
        'raw': '0x%08x' % raw,
        'slot': (raw >> 24) & 0x0F,
        'script': (raw >> 12) & 0x0FFF,
        'entry': raw & 0x0FFF,
    }


def read_utf8_buffer(emu, pointer):
    """Read the game's one-byte-length-prefixed UTF-8 dialogue buffer."""
    if pointer == 0:
        return ''
    try:
        size = emu.e.memory.unsigned.read_byte(pointer)
        # Dialogue buffers are small. Reject a bad pointer rather than reading
        # arbitrary emulated memory into an oracle result.
        if size > 0x400:
            return '<invalid length %d>' % size
        raw = bytes(emu.e.memory.unsigned.read_byte(pointer + i) for i in range(1, size + 1))
        return raw.decode('utf-8', 'replace')
    except Exception as exc:
        return '<unreadable: %s>' % type(exc).__name__


def snapshot(emu, event_ids):
    context = emu.read32(SCRIPT_CONTEXT_PTR)
    if context == 0:
        raise RuntimeError('Script_GetContext pointer is null')
    raw_ref = emu.read32(context + CURRENT_REFERENCE)
    reference = decode_reference(raw_ref)
    record = {
        'frame': emu.frames,
        'context': '0x%08x' % context,
        'reference': reference,
        'pc': '0x%08x' % emu.read32(context + CURRENT_PC),
        'command_word': '0x%08x' % emu.read32(context + CURRENT_COMMAND),
        'name': read_utf8_buffer(emu, emu.read32(context + NAME_BUFFER)),
        'sub': read_utf8_buffer(emu, emu.read32(context + SUB_BUFFER)),
        'text': read_utf8_buffer(emu, emu.read32(context + TEXT_BUFFER)),
    }
    key = (reference['slot'], reference['script'], reference['entry'])
    if key in event_ids:
        record['event_id'] = event_ids[key]
    return record


def changed(a, b):
    """Keep execution changes, but retain frame zero as the anchor."""
    return any(a[key] != b[key] for key in ('reference', 'pc', 'command_word', 'name', 'sub', 'text'))


def load_event_ids(content):
    path = os.path.join(content, 'scripts', 'directory.json')
    with open(path) as source:
        records = json.load(source)['records']
    result = {}
    for event_id, ref in records.items():
        # A reference can name multiple directory records. Preserve all of
        # them instead of pretending a runtime reference identifies one event.
        key = (ref['slot'], ref['script'], ref['entry'])
        result.setdefault(key, []).append('0x' + event_id)
    return result


def compare_text_trace(content, event_id, observations, max_yields):
    """Match observed rendered text against the deterministic bytecode trace.

    A match proves that a displayed line was reached by the same extracted
    bytecode path. It does *not* claim native-command or save-state parity:
    those commands remain part of the emulator's ground truth.
    """
    from event_trace import parse_event_id, trace

    normalized_id = parse_event_id(event_id)
    traced = trace(content, normalized_id, max_yields)
    lines = [event['text'] for event in traced['events'] if event['kind'] == 'text']
    cursor = 0
    matches, unmatched = [], []
    previous_rendered = None
    for observation_index, observation in enumerate(observations):
        text = observation['text']
        if not text:
            continue
        rendered = (observation['name'], observation['sub'], text)
        # One tap can finish typewriter/transition bookkeeping while leaving
        # the exact rendered line intact. PC/command changes are useful oracle
        # evidence, but must not require the bytecode trace to repeat text.
        if rendered == previous_rendered:
            continue
        previous_rendered = rendered
        try:
            trace_index = lines.index(text, cursor)
        except ValueError:
            unmatched.append({'observation': observation_index, 'text': text})
        else:
            matches.append({'observation': observation_index, 'trace_text_index': trace_index, 'text': text})
            cursor = trace_index + 1
    return {
        'event_id': '0x' + normalized_id,
        'trace_termination': traced['termination'],
        'traced_text_count': len(lines),
        'matched_in_order': matches,
        'unmatched': unmatched,
        'passed': bool(matches) and not unmatched,
        'scope': 'exact rendered text subsequence only; not native-command equivalence',
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('content', help='extracted content directory (for directory reference labels)')
    parser.add_argument('--rom', default='rom/loveplus_plus_j.nds', help='user-local ROM path')
    parser.add_argument('--state', required=True, help='user-local DeSmuME save-state path')
    parser.add_argument('--advances', type=int, default=0, help='number of dialogue taps to make')
    parser.add_argument('--settle-frames', type=int, default=12, help='frames after each tap before sampling')
    parser.add_argument('--tap-x', type=int, default=30)
    parser.add_argument('--tap-y', type=int, default=100)
    parser.add_argument('--compare-event', metavar='EVENT_ID',
                        help='match observed dialogue against event_trace.py for this event')
    parser.add_argument('--trace-max-yields', type=int, default=1000,
                        help='yield cap used with --compare-event')
    parser.add_argument('--json', metavar='OUT', help='write JSON to OUT instead of stdout')
    args = parser.parse_args()
    if args.advances < 0 or args.settle_frames < 0:
        parser.error('--advances and --settle-frames must be non-negative')
    if args.trace_max_yields <= 0:
        parser.error('--trace-max-yields must be positive')
    if not os.path.isfile(args.rom):
        parser.error('ROM not found: %s' % args.rom)
    if not os.path.isfile(args.state):
        parser.error('state not found: %s' % args.state)

    # Keep --help and pure parsing usable in the normal project Python; the
    # optional py-desmume dependency is needed only for an actual oracle run.
    from emu_driver import Emu
    emu = Emu(args.rom, state=args.state)
    event_ids = load_event_ids(args.content)
    observations = [snapshot(emu, event_ids)]
    previous = observations[0]
    for _ in range(args.advances):
        emu.touch(args.tap_x, args.tap_y)
        emu.step(args.settle_frames)
        current = snapshot(emu, event_ids)
        if changed(previous, current):
            observations.append(current)
        previous = current

    output = {
        'oracle': 'py-desmume live ScriptContext/dialogue buffers',
        'state': os.path.basename(args.state),
        'tap': {'x': args.tap_x, 'y': args.tap_y, 'count': args.advances},
        'settle_frames': args.settle_frames,
        'observations': observations,
    }
    if args.compare_event:
        output['comparison'] = compare_text_trace(content=args.content, event_id=args.compare_event,
                                                   observations=observations, max_yields=args.trace_max_yields)
    text = json.dumps(output, ensure_ascii=False, indent=1) + '\n'
    if args.json:
        with open(args.json, 'w') as out:
            out.write(text)
    else:
        print(text, end='')


if __name__ == '__main__':
    main()
