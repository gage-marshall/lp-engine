#!/usr/bin/env python3
"""Run one extracted LovePlus+ event as a deterministic, inspectable VM trace.

Usage:
    event_trace.py CONTENT_DIR EVENT_ID [--reply COMMAND=VALUE] [--text-reply MODE=VALUE]
                   [--max-yields N] [--json OUT.json]

This is intentionally a *reference host*, not a game implementation.  It
resolves the event through scripts/directory.json and scripts/resource_map.json,
then supplies conservative replies to native commands and mode-based text
choices so the bytecode can be observed. Commands are logged rather than
silently treated as implemented.
Repeated command bursts are reported as ``repeating_command_loop``.  They may
be task polling, event selection, or another native state-machine dependency;
the trace deliberately does not guess which.
"""
import argparse
import collections
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from lpvm import M32, Script, s16, s32

# Every opcode accepted by Script_ExecuteOpcode for the extracted SBIN format.
# Several sparse values are intentional no-ops in the native dispatcher; keep
# this explicit so corpus validation can detect a gap in this tracer.
SUPPORTED_OPCODES = (set(range(0x22)) |
                     {0x30, 0x31, 0x32, 0x33, 0x34, 0x35} |
                     set(range(0x40, 0x46)) |
                     set(range(0x50, 0x5d)) |
                     set(range(0x80, 0x89)))


class TraceHost:
    """Minimal deterministic answers for the few script-visible state effects.

    Unknown commands reply with zero.  That is deliberately conservative: the
    trace marks the result as a model, rather than claiming game-equivalent
    behavior.  The small flag model lets ordinary set/test branches remain
    readable in traces.
    """
    SET_FLAG = 0x55
    TEST_FLAG = 0x59

    def __init__(self, replies=None, text_replies=None):
        self.flags = set()
        self.commands = collections.Counter()
        self.events = []
        self.replies = replies or {}
        self.text_replies = text_replies or {}

    def command(self, command_id, args):
        self.commands[command_id] += 1
        record = {'kind': 'command', 'id': '0x%03x' % command_id, 'args': args}
        if command_id in self.replies:
            value = self.replies[command_id]
            record['model'] = 'configured_reply'
            record['reply'] = value
            self.events.append(record)
            return value
        if command_id == self.SET_FLAG and args:
            self.flags.add(args[0])
            record['model'] = 'set_flag'
        elif command_id == self.TEST_FLAG and args:
            value = int(args[0] in self.flags)
            record['model'] = 'test_flag'
            record['reply'] = value
            self.events.append(record)
            return value
        self.events.append(record)
        return 0

    def text(self, buffers, mode):
        def render(parts):
            return ''.join(text if kind == 's' else '{v%d}' % text for kind, text in parts)
        record = {'kind': 'text', 'name': render(buffers['name']),
                  'sub': render(buffers['sub']), 'text': render(buffers['text']),
                  'mode': mode}
        if mode in self.text_replies:
            record['model'] = 'configured_text_reply'
            record['reply'] = self.text_replies[mode]
            self.events.append(record)
            return self.text_replies[mode]
        self.events.append(record)
        return None


class TraceMachine:
    """SBIN interpreter with the game's master-directory call convention.

    Unlike ``Script.run()``, this interpreter owns the active script context.
    Opcode 0x16 looks up an event id in the slot-0 master directory, resolves
    its resource through ``resource_map.json``, and can therefore call into a
    different SBIN file before later returning to its caller.
    """
    def __init__(self, scripts_dir, directory, resource_map, initial_ref):
        self.scripts_dir = scripts_dir
        self.records = directory['records']
        self.pools = resource_map['slots']
        self.cache = {}
        self.current = self.load_ref(initial_ref)
        self.pc = self.current['script'].entries[initial_ref['entry']]
        self.acc = self.flag = 0
        self.stack = []
        self.frames = []
        self.regs = {}
        self.gvars = {}
        self.local_vars = collections.defaultdict(dict)
        self.bufs = [[], [], []]
        # The native VM stores a target reference in register 1 for opcode
        # 0x14, then unwinds normally before Script_Run calls that reference.
        # Keep it separately because this tracer's compact frame model has no
        # root Script_CallRef frame.
        self.pending_return_event = None

    def load_ref(self, ref):
        slot, script_id, entry = ref['slot'], ref['script'], ref['entry']
        pool = self.pools.get(str(slot))
        if pool is None or script_id >= len(pool['entries']):
            raise ValueError('invalid script reference slot/script %d/%d' % (slot, script_id))
        resource = pool['entries'][script_id]
        key = (slot, script_id)
        if key not in self.cache:
            path = os.path.join(self.scripts_dir, '%04d.sbin' % resource)
            self.cache[key] = {'slot': slot, 'script_id': script_id, 'resource': resource,
                               'script': Script(dec=open(path, 'rb').read())}
        loaded = self.cache[key]
        if entry >= len(loaded['script'].entries):
            raise ValueError('%04d.sbin entry %d is out of range' % (resource, entry))
        return loaded

    def current_ref(self, entry=None):
        return {'slot': self.current['slot'], 'script': self.current['script_id'],
                'entry': entry, 'resource_file': '%04d.sbin' % self.current['resource']}

    def call(self, event_id, return_pc):
        ref = self.records.get('%08x' % event_id)
        if ref is None:
            return None
        target = self.load_ref(ref)
        self.frames.append((self.current, return_pc))
        self.current = target
        self.pc = target['script'].entries[ref['entry']]
        return self.current_ref(ref['entry'])

    def enter(self, event_id):
        """Begin an event without manufacturing a caller frame.

        This models the final Script_Run handoff after a ``return by id`` has
        unwound the original call chain.
        """
        ref = self.records.get('%08x' % event_id)
        if ref is None:
            return None
        self.current = self.load_ref(ref)
        self.pc = self.current['script'].entries[ref['entry']]
        return self.current_ref(ref['entry'])

    def finish_return(self):
        """Return one frame, or perform a deferred opcode-14 handoff."""
        if self.frames:
            self.current, self.pc = self.frames.pop()
            return None
        if self.pending_return_event is not None:
            event_id = self.pending_return_event
            self.pending_return_event = None
            target = self.enter(event_id)
            if target is None:
                return ('unresolved_call', event_id)
            return ('call', event_id, target)
        return ('end', self.acc)

    def run(self, max_steps=200000):
        steps = 0
        while True:
            steps += 1
            if steps > max_steps:
                yield ('limit', self.pc); return
            data = self.current['script'].d
            if self.pc < 0 or self.pc + 2 > len(data):
                yield ('fault', 'instruction outside script', self.pc); return
            op, count = data[self.pc], data[self.pc + 1]
            end = self.pc + 2 + 2 * count
            if end > len(data):
                yield ('fault', 'truncated instruction', self.pc); return
            args = struct.unpack('>%dH' % count, data[self.pc + 2:end]) if count else ()
            nxt = end
            if op == 0x00:
                pass
            elif op == 0x01:
                self.stack.append(self.acc)
            elif op == 0x02:
                self.acc = self.stack.pop() if self.stack else 0
            elif op == 0x03:
                for _ in range(args[0]):
                    if self.stack: self.stack.pop()
            elif op == 0x10:
                nxt += s16(args[0])
            elif op == 0x11:
                if self.flag: nxt += s16(args[0])
            elif op == 0x12:
                if not self.flag: nxt += s16(args[0])
            elif op == 0x13:
                outcome = self.finish_return()
                if outcome is None:
                    continue
                yield outcome
                if outcome[0] == 'end' or outcome[0] == 'unresolved_call':
                    return
                continue
            elif op == 0x14:
                event_id = (args[0] << 16) | args[1]
                yield ('return_by_id', event_id)
                self.pending_return_event = event_id
                outcome = self.finish_return()
                if outcome is None:
                    continue
                yield outcome
                if outcome[0] == 'end' or outcome[0] == 'unresolved_call':
                    return
                continue
            elif op == 0x16:
                event_id = (args[0] << 16) | args[1]
                target = self.call(event_id, nxt)
                if target is None:
                    yield ('unresolved_call', event_id); return
                yield ('call', event_id, target)
                continue
            elif op == 0x20:
                self.flag = int(self.acc != 0)
            elif op == 0x21:
                self.flag = int(bool(self.stack) and self.acc == self.stack[-1])
            elif op == 0x30:
                self.acc = 0
            elif op == 0x31:
                self.acc = 1
            elif op == 0x32:
                self.acc = args[0]
            elif op == 0x33:
                self.acc = (args[0] << 16) | args[1]
            elif op == 0x34:
                self.acc = int(self.acc != 0)
            elif op == 0x35:
                self.acc = (-self.acc) & M32
            elif op == 0x40:
                self.acc = self.regs.get(args[0], 0)
            elif op == 0x41:
                self.acc = self.gvars.get(args[0], 0)
            elif op == 0x42:
                self.acc = self.local_vars[(self.current['slot'], self.current['script_id'])].get(args[0], 0)
            elif op == 0x43:
                self.regs[args[0]] = self.acc
            elif op == 0x44:
                self.gvars[args[0]] = self.acc
            elif op == 0x45:
                self.local_vars[(self.current['slot'], self.current['script_id'])][args[0]] = self.acc
            elif 0x50 <= op <= 0x5c:
                b = self.stack.pop() if self.stack else 0
                a = self.stack.pop() if self.stack else 0
                if op == 0x50: self.acc = (a + b) & M32
                elif op == 0x51: self.acc = (a - b) & M32
                elif op == 0x52: self.acc = (a * b) & M32
                elif op == 0x53: self.acc = 0 if b == 0 else (int(abs(s32(a)) // abs(s32(b))) * (1 if (s32(a) < 0) == (s32(b) < 0) else -1)) & M32
                elif op == 0x54: self.acc = a & b
                elif op == 0x55: self.acc = a | b
                elif op == 0x56: self.acc = a ^ b
                elif op == 0x57: self.acc = int(a == b)
                elif op == 0x58: self.acc = int(a != b)
                elif op == 0x59: self.acc = int(a < b)
                elif op == 0x5a: self.acc = int(a <= b)
                elif op == 0x5b: self.acc = int(a > b)
                elif op == 0x5c: self.acc = int(a >= b)
            elif op == 0x80:
                self.bufs[args[0]] = []
            elif op == 0x81:
                self.bufs[args[0]].append(('s', self.current['script'].string((args[1] << 16) | args[2])))
            elif op == 0x82:
                self.bufs[args[0]].append(('v', args[1]))
            elif op == 0x83:
                yield ('text', {'text': list(self.bufs[0]), 'sub': list(self.bufs[1]), 'name': list(self.bufs[2])}, None)
            elif op == 0x84:
                reply = yield ('text', {'text': list(self.bufs[0]), 'sub': list(self.bufs[1]), 'name': list(self.bufs[2])}, args[0])
                # Native opcode 0x84 resumes through its route state machine.
                # Mode-2 choice screens write the chosen one-based result to
                # register 0; accepting an explicit response lets the tracer
                # follow that bytecode path while preserving the model label.
                if reply is not None:
                    self.regs[0] = reply & M32
            elif op == 0x87:
                arity = self.stack[-1] if self.stack else 0
                command_args = self.stack[-1 - arity:-1] if arity else []
                reply = yield ('cmd', (args[0] << 16) | args[1], list(command_args))
                if reply is not None: self.acc = reply & M32
            elif op == 0x88:
                yield ('wait_fade',)
            elif op in (0x85, 0x86) or 0x5d <= op <= 0x7f or op in (0x04, 0x15, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f):
                pass
            else:
                yield ('badop', op, self.pc); return
            self.pc = nxt


def parse_event_id(value):
    try:
        return '%08x' % int(value, 0)
    except ValueError as exc:
        raise argparse.ArgumentTypeError('event id must be an integer, e.g. 0xa063f380') from exc


def resolve(content, event_id):
    scripts = os.path.join(content, 'scripts')
    directory = json.load(open(os.path.join(scripts, 'directory.json')))
    ref = directory['records'].get(event_id)
    if ref is None:
        raise KeyError('event id %s is not in scripts/directory.json' % event_id)
    resource_map = json.load(open(os.path.join(scripts, 'resource_map.json')))
    pool = resource_map['slots'].get(str(ref['slot']))
    if pool is None or ref['script'] >= len(pool['entries']):
        raise ValueError('invalid resource-map reference for event %s' % event_id)
    resource = pool['entries'][ref['script']]
    path = os.path.join(scripts, '%04d.sbin' % resource)
    return ref, resource, path


def trace(content, event_id, max_yields, replies=None, text_replies=None):
    ref, resource, path = resolve(content, event_id)
    scripts_dir = os.path.join(content, 'scripts')
    directory = json.load(open(os.path.join(scripts_dir, 'directory.json')))
    resource_map = json.load(open(os.path.join(scripts_dir, 'resource_map.json')))
    machine = TraceMachine(scripts_dir, directory, resource_map, ref)
    initial_entry_count = len(machine.current['script'].entries)

    host = TraceHost(replies, text_replies)
    generator = machine.run(max_steps=max_yields * 32)
    reply = None
    termination = 'yield_limit'
    # Missing native state often produces the same short run forever.  Four
    # repeats of a burst up to four commands long is useful evidence, while
    # keeping the trace concise without guessing the subsystem responsible.
    recent = collections.deque(maxlen=16)
    for _ in range(max_yields):
        event = generator.send(reply)
        reply = None
        if event[0] == 'cmd':
            command_id, args = event[1], event[2]
            recent.append(('cmd', command_id, tuple(args)))
            reply = host.command(command_id, args)
            for period in range(1, 5):
                span = period * 4
                if len(recent) >= span:
                    tail = list(recent)[-span:]
                    if tail == tail[:period] * 4:
                        termination = 'repeating_command_loop'
                        break
            if termination == 'repeating_command_loop': break
        elif event[0] == 'text':
            recent.append(('text',))
            reply = host.text(event[1], event[2])
        elif event[0] == 'wait_fade':
            recent.append(('wait_fade',))
            host.events.append({'kind': 'wait_fade'})
        elif event[0] == 'call':
            recent.append(('call', event[1]))
            host.events.append({'kind': 'call', 'event_id': '%08x' % event[1], 'target': event[2]})
        elif event[0] == 'return_by_id':
            host.events.append({'kind': 'return_by_id', 'event_id': '%08x' % event[1]})
        elif event[0] == 'end':
            host.events.append({'kind': 'end', 'result': event[1]})
            termination = 'return'
            break
        else:
            host.events.append({'kind': event[0], 'detail': list(event[1:])})
            termination = event[0]
            break

    return {'event_id': event_id, 'reference': ref, 'resource_file': '%04d.sbin' % resource,
            'entry_count': initial_entry_count,
            'host_model': 'unknown commands reply 0; flag set/test modeled; explicit replies override both',
            'configured_replies': {'0x%03x' % command: value for command, value in sorted((replies or {}).items())},
            'configured_text_replies': {'%d' % mode: value for mode, value in sorted((text_replies or {}).items())},
            'termination': termination, 'command_counts': {'0x%03x' % key: value for key, value in sorted(host.commands.items())},
            'events': host.events}


def parse_reply(value):
    try:
        command, reply = value.split('=', 1)
        return int(command, 0), int(reply, 0)
    except ValueError as exc:
        raise argparse.ArgumentTypeError('reply must be COMMAND=VALUE, e.g. 0x3e=1') from exc


def parse_text_reply(value):
    try:
        mode, reply = value.split('=', 1)
        return int(mode, 0), int(reply, 0)
    except ValueError as exc:
        raise argparse.ArgumentTypeError('text reply must be MODE=VALUE, e.g. 2=1') from exc


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('content')
    parser.add_argument('event_id', type=parse_event_id)
    parser.add_argument('--reply', action='append', type=parse_reply, default=[], metavar='COMMAND=VALUE',
                        help='override one command result; may be specified multiple times')
    parser.add_argument('--text-reply', action='append', type=parse_text_reply, default=[], metavar='MODE=VALUE',
                        help='set register 0 after a mode-based text commit (e.g. 2=1); may be specified multiple times')
    parser.add_argument('--max-yields', type=int, default=1000)
    parser.add_argument('--json', metavar='OUT')
    args = parser.parse_args()
    if args.max_yields <= 0:
        parser.error('--max-yields must be positive')
    try:
        result = trace(args.content, args.event_id, args.max_yields, dict(args.reply), dict(args.text_reply))
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as exc:
        parser.exit(2, 'error: %s\n' % exc)
    text = json.dumps(result, ensure_ascii=False, indent=1)
    if args.json:
        with open(args.json, 'w') as f:
            f.write(text + '\n')
    print(text)


if __name__ == '__main__':
    main()
