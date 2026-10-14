"""Runs a KYNEX program on Doosan's emulator and compares the time each move
takes there, and where the tool ends it, with what KYNEX plans.

    docker run -d --rm --privileged --name kynex_emulator \\
        -e ROBOT_MODEL=M1013 -p 12345:12345 doosanrobot/dsr_emulator:3.0.1
    docker build -t kynex-run-drl tools/doosan
    cargo test -p kynex doosan_stage_one -- --ignored
    python tools/doosan/check.py

The program comes from `target/doosan/<name>.drl`, the plan from
`<name>.json`. The program is started by `run_drl` (see run_drl.cpp), which
registers its tool point first. What it does is read from the emulator's
log: each move's `tp_log` as it starts and as it is done, the latter with
the joints and the tool pose the controller has then, all on the
controller's clock. The first move, from wherever the emulator stands to
the program's start, is not timed: KYNEX does not know where that is.

Exits non-zero when the moves, together, are off by more than the
tolerance (10 % by default), or a move ends more than 2 mm from where KYNEX
says, or the program stops before its end.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
RUNNER = 'kynex_run_drl'
LINE = re.compile(r'^\[(\d+(?:\.\d+)?)\](.*)$')
NUMBERS = re.compile(r'\[([^\]]*)\]')


def docker(*args, check=True):
    return subprocess.run(['docker', *args], capture_output=True, text=True,
                          encoding='utf-8', errors='replace', check=check)


def run(args, name):
    """Starts the program and returns the emulator's log lines from then
    to the program's end, each with its time."""
    docker('rm', '-f', RUNNER, check=False)
    since = time.time() - 1
    docker('run', '-d', '--name', RUNNER, '--network', f'container:{args.emulator}',
           '-v', f'{args.dir}:/work', 'kynex-run-drl', '127.0.0.1', f'/work/{name}.drl',
           str(int(args.timeout)))
    deadline = time.time() + args.timeout
    lines = []
    try:
        while time.time() < deadline:
            time.sleep(2)
            log = docker('logs', '--since', f'{since:.0f}', args.emulator).stdout
            lines = []
            for raw in log.splitlines():
                m = LINE.match(raw.strip())
                if m:
                    lines.append((float(m.group(1)), m.group(2)))
                elif 'msg' in raw or 'Exception' in raw:
                    lines.append((None, raw.strip()))
            if any('#PROGRAM END' in text for _, text in lines):
                break
    finally:
        print(docker('logs', RUNNER, check=False).stdout.strip())
        docker('rm', '-f', RUNNER, check=False)
    return lines


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--dir', default=os.path.join(ROOT, 'target', 'doosan'))
    ap.add_argument('--name', default='stage1',
                    help='the program in --dir to run: NAME.drl and NAME.json')
    ap.add_argument('--emulator', default='kynex_emulator',
                    help="the emulator's container")
    ap.add_argument('--tolerance', type=float, default=0.10)
    ap.add_argument('--timeout', type=float, default=900.0)
    args = ap.parse_args()

    plan = json.load(open(os.path.join(args.dir, f'{args.name}.json'), encoding='utf-8'))
    planned = plan['moves']
    ends = plan['ends']
    lines = run(args, args.name)

    marks, done, at = {}, {}, {}
    for t, text in lines:
        m = re.search(r'Log Message : kynex (mark|done) (\d+)(.*)', text)
        if not m:
            continue
        k = int(m.group(2))
        if m.group(1) == 'mark':
            marks.setdefault(k, t)
        else:
            done.setdefault(k, t)
            # Its joints, then the tool pose: millimetres, then angles.
            numbers = NUMBERS.findall(m.group(3))
            if len(numbers) >= 2:
                at[k] = [float(v) for v in numbers[1].split(',')][:3]
    end = [text for _, text in lines if '#PROGRAM END' in text]
    trouble = [text for _, text in lines
               if 'Exception' in text or 'msg' in text or '#WARN' in text]
    for text in trouble:
        print(text)
    last = len(planned) + 2
    if last not in marks:
        print(f'the program stopped at mark {max(marks or [0])}: {end}')
        return 2

    print(f'{"move":<6} {"kind":<9} {"KYNEX s":>8} {"DRCF s":>8} {"off":>7}')
    total_k = total_d = 0.0
    # Mark n+1 is move n (0 is the move to the start), logged as it starts
    # and once it is done, so that what the program does between moves is
    # not counted as moving; KYNEX's move is timed to where the next starts.
    for i, move in enumerate(planned, start=1):
        d = done[i + 1] - marks[i + 1]
        if i < len(planned):
            k = planned[i]['start'] - move['start']
        else:
            k = plan['duration'] - move['start']
        total_k += k
        total_d += d
        print(f'{move["name"]:<6} {move["kind"]:<9} {k:8.3f} {d:8.3f} {(k - d) / d * 100:+6.1f}%')
    off = (total_k - total_d) / total_d
    print(f'{"all":<6} {"":<9} {total_k:8.3f} {total_d:8.3f} {off * 100:+6.1f}%')
    timed = abs(off) <= args.tolerance

    worst = 0.0
    print()
    print(f'{"move":<5} {"KYNEX (mm)":>28} {"DRCF (mm)":>28} {"off":>7}')
    for i, want in enumerate(ends):
        got = at.get(i + 1)
        want = [v * 1000 for v in want]
        if got is None:
            print(f'{i + 1:<5} no pose logged')
            worst = float('inf')
            continue
        off_mm = sum((a - b) ** 2 for a, b in zip(want, got)) ** 0.5
        worst = max(worst, off_mm)
        fmt = lambda p: ', '.join(f'{v:8.1f}' for v in p)
        print(f'{i + 1:<5} {fmt(want):>28} {fmt(got):>28} {off_mm:6.2f}')
    placed = worst <= 2.0
    print(f'time: {"within" if timed else "outside"} {args.tolerance * 100:.0f} %; '
          f'worst position {worst:.2f} mm: {"within" if placed else "outside"} 2 mm')
    return 0 if timed and placed and not trouble else 1


if __name__ == '__main__':
    sys.exit(main())
