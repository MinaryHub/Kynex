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


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--host', default='127.0.0.1')
    ap.add_argument('--dir', default=os.path.join(ROOT, 'target', 'ursim'))
    ap.add_argument('--tolerance', type=float, default=0.10)
    ap.add_argument('--timeout', type=float, default=120.0)
    args = ap.parse_args()

    script = open(os.path.join(args.dir, 'm1.script'), encoding='utf-8').read()
    # Sent to the controller, a function runs as it is defined; the call
    # after it, for a file, would not parse.
    script = script[: script.rindex('end\n') + 4]
    plan = json.load(open(os.path.join(args.dir, 'm1.json'), encoding='utf-8'))
    planned = plan['moves']

    ready(args.host)
    dashboard(args.host, ['close safety popup', 'unlock protective stop'])
    rtde = Rtde(args.host)
    rtde.setup(['timestamp', 'output_int_register_0', 'runtime_state'])
    rtde.start()
    # The register as the last run left it: not a mark of this one.
    start = rtde.sample()
    sender = socket.create_connection((args.host, 30002), 5)
    sender.sendall(script.encode())
    sender.close()

    # The controller's time each number first shows.
    marks = {}
    last = len(planned) + 2
    deadline = time.time() + args.timeout
    seen_running = False
    while time.time() < deadline:
        s = rtde.sample()
        reg = s['output_int_register_0']
        # The last run's number counts once this run has written its own.
        if reg not in marks and (marks or reg != start['output_int_register_0']):
            marks[reg] = s['timestamp']
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
    # Mark n+1 starts move n (0 is the move to the start); the controller
    # writes the next once the move is done — or, into a blend, once the
    # next move takes over. KYNEX's move is timed the same way: to where
    # the next starts, which a blend brings forward.
    for i, move in enumerate(planned, start=1):
        u = marks[i + 2] - marks[i + 1]
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
