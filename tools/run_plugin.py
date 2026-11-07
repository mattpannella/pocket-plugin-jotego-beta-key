#!/usr/bin/env python3
"""Run a Pocket plugin (plugin.wasm) headless, the way Pocket Sync and pupdate do: the SD card
folder is the plugin's pocket/, each Choice/Text is answered from --answer in order, and the
plugin's log goes to stdout. Needs the extism Python host (python3 -m pip install extism).

  python3 tools/run_plugin.py target/wasm32-wasip1/release/plugin.wasm /tmp/sd --answer "1:" --answer "3:" ...

The card folder is the SD card root (or any scratch folder; it is created if missing).

A Choice is matched by the start of the option text; a Text prompt takes the answer verbatim.
--kill answers the Nth prompt (1-based) with Kill instead.
"""
import argparse
import json
import sys
from pathlib import Path

import extism


@extism.host_fn(name='print_msg', signature=([extism.ValType.I64], [extism.ValType.I64]))
def print_msg(plugin, inputs, outputs):
    sys.stdout.write(plugin.input_string(inputs[0]))
    sys.stdout.flush()


@extism.host_fn(name='open_url', signature=([extism.ValType.I64], [extism.ValType.I64]))
def open_url(plugin, inputs, outputs):
    print(f'[open_url {plugin.input_string(inputs[0])}]')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('wasm', type=Path)
    parser.add_argument('card', type=Path)
    parser.add_argument('--answer', action='append', default=[], help='answer per prompt, in order (default: first choice)')
    parser.add_argument('--kill', type=int, default=0, help='answer prompt N (1-based) with Kill')
    parser.add_argument('--host-folder', type=Path, help="the plugin's own folder on the computer (host/)")
    parser.add_argument('--config', action='append', default=[], metavar='KEY=VALUE')
    parser.add_argument('--allow-host', action='append', default=[], metavar='HOST',
                        help='host the plugin may call (the updater takes these from plugin.json)')
    args = parser.parse_args()
    if not args.wasm.is_file():
        raise SystemExit(f'{args.wasm}: not found (build it first: scripts/build.sh)')
    if not args.card.is_dir():
        print(f'[creating empty card folder {args.card}]')
        args.card.mkdir(parents=True)
    paths = {str(args.card.resolve()): 'pocket'}
    if args.host_folder:
        paths[str(args.host_folder.resolve())] = 'host'
    manifest = {'wasm': [{'path': str(args.wasm.resolve())}], 'allowed_paths': paths,
                'allowed_hosts': args.allow_host,
                'config': dict(c.split('=', 1) for c in args.config)}
    answers = list(args.answer)
    prompt_no = 0
    with extism.Plugin(manifest, wasi=True, functions=[print_msg, open_url]) as plugin:
        message = json.loads(plugin.call('start', b''))
        while message != 'Exit':
            prompt_no += 1
            (kind, body), = message.items()
            if prompt_no == args.kill:
                print(f'[{body["query"]}: Kill]')
                reply = 'Kill'
            elif kind == 'Choice':
                want = answers.pop(0) if answers else body['choices'][0]
                value = next((c for c in body['choices'] if c.startswith(want)), None)
                if value is None:
                    raise SystemExit(f'{want!r} is not one of {body["choices"]}')
                print('  choices: ' + ' | '.join(body['choices']))
                print(f'[{body["query"]}: {value}]')
                reply = {'Answer': {'name': body['name'], 'value': value}}
            elif kind == 'Text':
                value = answers.pop(0) if answers else ''
                print(f'[{body["query"]}: {value}]')
                reply = {'Answer': {'name': body['name'], 'value': value}}
            else:
                raise SystemExit(f'unexpected {kind} message: {body}')
            message = json.loads(plugin.call('handle_response', json.dumps(reply).encode()))


if __name__ == '__main__':
    main()
