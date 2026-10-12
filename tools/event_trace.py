#!/usr/bin/env python3
"""Run one extracted LovePlus+ event as a deterministic, inspectable VM trace.

Usage:
    event_trace.py CONTENT_DIR EVENT_ID [--reply COMMAND=VALUE] [--max-yields N] [--json OUT.json]

This is intentionally a *reference host*, not a game implementation.  It
resolves the event through scripts/directory.json and scripts/resource_map.json,
then supplies conservative replies to native commands so the bytecode can be
observed.  Commands are logged rather than silently treated as implemented.
Repeated command bursts are reported as ``repeating_command_loop``.  They may
be task polling, event selection, or another native state-machine dependency;
the trace deliberately does not guess which.
"""
import argparse
import collections
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from lpvm import Script


class TraceHost:
    """Minimal deterministic answers for the few script-visible state effects.

    Unknown commands reply with zero.  That is deliberately conservative: the
    trace marks the result as a model, rather than claiming game-equivalent
    behavior.  The small flag model lets ordinary set/test branches remain
    readable in traces.
    """
    SET_FLAG = 0x55
    TEST_FLAG = 0x59

    def __init__(self, replies=None):
        self.flags = set()
        self.commands = collections.Counter()
        self.events = []
        self.replies = replies or {}

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
        self.events.append({'kind': 'text', 'name': render(buffers['name']),
                            'sub': render(buffers['sub']), 'text': render(buffers['text']),
                            'mode': mode})


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


def trace(content, event_id, max_yields, replies=None):
    ref, resource, path = resolve(content, event_id)
    script = Script(dec=open(path, 'rb').read())
    if ref['entry'] >= len(script.entries):
        raise ValueError('%s entry %d is outside %04d.sbin' % (event_id, ref['entry'], resource))

    host = TraceHost(replies)
    generator = script.run(ref['entry'], max_steps=max_yields * 32)
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
            host.text(event[1], event[2])
        elif event[0] == 'wait_fade':
            recent.append(('wait_fade',))
            host.events.append({'kind': 'wait_fade'})
        elif event[0] == 'end':
            host.events.append({'kind': 'end', 'result': event[1]})
            termination = 'return'
            break
        else:
            host.events.append({'kind': event[0], 'detail': list(event[1:])})
            termination = event[0]
            break

    return {'event_id': event_id, 'reference': ref, 'resource_file': '%04d.sbin' % resource,
            'entry_count': len(script.entries),
            'host_model': 'unknown commands reply 0; flag set/test modeled; explicit replies override both',
            'configured_replies': {'0x%03x' % command: value for command, value in sorted((replies or {}).items())},
            'termination': termination, 'command_counts': {'0x%03x' % key: value for key, value in sorted(host.commands.items())},
            'events': host.events}


def parse_reply(value):
    try:
        command, reply = value.split('=', 1)
        return int(command, 0), int(reply, 0)
    except ValueError as exc:
        raise argparse.ArgumentTypeError('reply must be COMMAND=VALUE, e.g. 0x3e=1') from exc


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('content')
    parser.add_argument('event_id', type=parse_event_id)
    parser.add_argument('--reply', action='append', type=parse_reply, default=[], metavar='COMMAND=VALUE',
                        help='override one command result; may be specified multiple times')
    parser.add_argument('--max-yields', type=int, default=1000)
    parser.add_argument('--json', metavar='OUT')
    args = parser.parse_args()
    if args.max_yields <= 0:
        parser.error('--max-yields must be positive')
    try:
        result = trace(args.content, args.event_id, args.max_yields, dict(args.reply))
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as exc:
        parser.exit(2, 'error: %s\n' % exc)
    text = json.dumps(result, ensure_ascii=False, indent=1)
    if args.json:
        with open(args.json, 'w') as f:
            f.write(text + '\n')
    print(text)


if __name__ == '__main__':
    main()
