"""Runs KYNEX's M1 tending program on URSim and compares the time each move
takes there with the time KYNEX plans for it.

    docker run -d --rm --name kynex-ursim -e ROBOT_MODEL=UR10E \\
        -p 30001-30004:30001-30004 -p 29999:29999 -p 6080:6080 \\
        universalrobots/ursim_e-series
    cargo test -p kynex ursim_program -- --ignored
    python tools/ursim/check.py

The program comes from `target/ursim/m1.script`, its moves numbered on
output integer register 0; the plan from `target/ursim/m1.json`. Times are
the controller's own clock, read over RTDE at 500 Hz. The first move, from
wherever URSim stands to the program's start, is not compared: KYNEX does
not know where that is.

The robot is powered on and its brakes released over the dashboard server
(port 29999) first. Exits non-zero when the program's moves, together, are
off by more than the tolerance (10 % by default).
"""

import argparse
import json
import os
import socket
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from rtde import Rtde  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))


def dashboard(host, commands):
    s = socket.create_connection((host, 29999), 5)
    s.recv(200)
    out = []
    for c in commands:
        s.sendall((c + '\n').encode())
        out.append(s.recv(500).decode().strip())
    s.close()
    return out


def ready(host):
    """Powered, brakes released, nothing else running."""
    dashboard(host, ['close popup', 'stop', 'power on'])
    for _ in range(60):
        mode = dashboard(host, ['robotmode'])[0]
        if 'IDLE' in mode or 'RUNNING' in mode:
            break
        time.sleep(0.5)
    dashboard(host, ['brake release'])
    for _ in range(60):
        mode = dashboard(host, ['robotmode'])[0]
        if 'RUNNING' in mode:
            return
        time.sleep(0.5)
    raise RuntimeError(f'URSim is not running: {mode}')


def send(host, path):
    """Sends a script file to the controller, to run now."""
    script = open(path, encoding='utf-8').read()
    # Sent to the controller, a function runs as it is defined; the call
    # after it, for a file, would not parse.
    script = script[: script.rindex('end\n') + 4]
    sender = socket.create_connection((host, 30002), 5)
    sender.sendall(script.encode())
    sender.close()


def check_io(args):
    """The wired program (`m1_io`): it runs through without a fault, its
    gripper output goes on and off, and the tool ends each move where KYNEX
    says — on the pallets' next places the second time round. Exits
    non-zero otherwise."""
    name = args.name or 'm1_io'
    expected = json.load(open(os.path.join(args.dir, f'{name}.json'), encoding='utf-8'))
    ends = expected['ends']
    bit = 1 << expected['gripper_output']

    ready(args.host)
    dashboard(args.host, ['close safety popup', 'unlock protective stop'])
    rtde = Rtde(args.host)
    rtde.setup(['timestamp', 'output_int_register_0', 'runtime_state',
                'actual_TCP_pose', 'actual_digital_output_bits'])
    rtde.start()
    start = rtde.sample()
    send(args.host, os.path.join(args.dir, f'{name}.script'))

    # Where the tool is as each number first shows: the end of the move
    # before it.
    at = {}
    last = len(ends) + 1
    closes = opens = 0
    gripping = bool(start['actual_digital_output_bits'] & bit)
    deadline = time.time() + args.timeout
    seen_running = False
    while time.time() < deadline:
        s = rtde.sample()
        reg = s['output_int_register_0']
        if reg not in at and (at or reg != start['output_int_register_0']):
            at[reg] = s['actual_TCP_pose'][:3]
        on = bool(s['actual_digital_output_bits'] & bit)
        if on != gripping:
            closes += on
            opens += not on
            gripping = on
        seen_running |= s['runtime_state'] == 2
        if last in at:
            break
        if seen_running and s['runtime_state'] in (0, 1):
            break
    rtde.close()
    if last not in at:
        print(f'the program stopped at mark {max(at or [0])}: '
              f'{dashboard(args.host, ["safetystatus", "robotmode"])}')
        return 2

    worst = 0.0
    print(f'{"move":<5} {"KYNEX (mm)":>28} {"URSim (mm)":>28} {"off":>7}')
    # Mark i + 2 is written once move i is done.
    for i, want in enumerate(ends):
        got = at[i + 2]
        off = sum((a - b) ** 2 for a, b in zip(want, got)) ** 0.5
        worst = max(worst, off)
        fmt = lambda p: ', '.join(f'{v * 1000:8.1f}' for v in p)
        print(f'{i + 1:<5} {fmt(want):>28} {fmt(got):>28} {off * 1000:6.2f}')
    ok = worst <= 0.002 and closes >= 1 and opens >= 1
    print(f'gripper output: on {closes}, off {opens}')
    print(f'worst {worst * 1000:.2f} mm: {"within" if ok else "outside"} 2 mm')
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--host', default='127.0.0.1')
    ap.add_argument('--dir', default=os.path.join(ROOT, 'target', 'ursim'))
    ap.add_argument('--tolerance', type=float, default=0.10)
    ap.add_argument('--timeout', type=float, default=120.0)
    ap.add_argument('--io', action='store_true',
                    help='the wired program: I/O and pallet places (m1_io)')
    ap.add_argument('--name', help="the program in --dir to run, NAME.script and NAME.json: "
                    "m1, or m1_io with --io; stage1 for stage 1's cell")
    args = ap.parse_args()
    if args.io:
        return check_io(args)

    name = args.name or 'm1'
    script = open(os.path.join(args.dir, f'{name}.script'), encoding='utf-8').read()
    # Sent to the controller, a function runs as it is defined; the call
    # after it, for a file, would not parse.
    script = script[: script.rindex('end\n') + 4]
    plan = json.load(open(os.path.join(args.dir, f'{name}.json'), encoding='utf-8'))
    planned = plan['moves']

    ready(args.host)
    dashboard(args.host, ['close safety popup', 'unlock protective stop'])
    rtde = Rtde(args.host)
    rtde.setup(['timestamp', 'output_int_register_0', 'output_int_register_1',
                'runtime_state'])
    rtde.start()
    # The registers as the last run left them: not marks of this one.
    start = rtde.sample()
    sender = socket.create_connection((args.host, 30002), 5)
    sender.sendall(script.encode())
    sender.close()

    # The controller's time each number first shows: on register 0 as a
    # move starts, on register 1 as it is done.
    marks = {}
    ends = {}
    last = len(planned) + 2
    deadline = time.time() + args.timeout
    seen_running = False
    while time.time() < deadline:
        s = rtde.sample()
        reg = s['output_int_register_0']
        # The last run's number counts once this run has written its own.
        if reg not in marks and (marks or reg != start['output_int_register_0']):
            marks[reg] = s['timestamp']
        done = s['output_int_register_1']
        if marks and done not in ends and (ends or done != start['output_int_register_1']):
            ends[done] = s['timestamp']
        seen_running |= s['runtime_state'] == 2
        if last in marks:
            break
        if seen_running and s['runtime_state'] in (0, 1) and reg != last:
            # Stopped before the end: a fault or a protective stop.
            break
    rtde.close()
    if last not in marks:
        print(f'the program stopped at mark {max(marks or [0])}: '
              f'{dashboard(args.host, ["safetystatus", "robotmode"])}')
        return 2

    print(f'{"move":<6} {"kind":<9} {"KYNEX s":>8} {"URSim s":>8} {"off":>7}')
    total_k = total_u = 0.0
    # Mark n+1 is move n (0 is the move to the start): written on register
    # 0 as it starts and on register 1 once it is done — or, into a blend,
    # once the next move takes over — so that what the program does
    # between moves (a gripper's time, a pulse to a machine) is not counted
    # as moving. KYNEX's move is timed the same way: to where the next
    # starts, which a blend brings forward.
    for i, move in enumerate(planned, start=1):
        u = ends[i + 1] - marks[i + 1]
        if i < len(planned):
            k = planned[i]['start'] - move['start']
        else:
            k = plan['duration'] - move['start']
        total_k += k
        total_u += u
        print(f'{move["name"]:<6} {move["kind"]:<9} {k:8.3f} {u:8.3f} {(k - u) / u * 100:+6.1f}%')
    off = (total_k - total_u) / total_u
    print(f'{"all":<6} {"":<9} {total_k:8.3f} {total_u:8.3f} {off * 100:+6.1f}%')
    ok = abs(off) <= args.tolerance
    print(f'{"within" if ok else "outside"} {args.tolerance * 100:.0f} %')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
