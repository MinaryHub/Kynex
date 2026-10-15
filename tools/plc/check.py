"""Virtual commissioning over Modbus TCP, checked against a server that is
not KYNEX's own: pymodbus (free) plays a PLC, and a small PLC program in
this script runs a CNC lathe in a running KYNEX through its cycle.

    pip install pymodbus
    kynex view --empty --port 8097 --project target/plc/kynex-project.json
    python tools/plc/check.py

The script places a lathe in that KYNEX (over its page's requests), wires
the lathe's commands to coils 0-4 and its states to coils 100-105, links
KYNEX to the PLC, and runs its program: open the door, close it, clamp,
unclamp, over and over. Each step's command coil is turned on, held until
the state coil says it is done, then turned off, as a PLC program's
outputs would be. It reports how long each step took: the door takes the
lathe's 3 s, the chuck its 1 s, give or take KYNEX's 50 ms exchange.
"""

import argparse
import asyncio
import json
import sys
import time
import urllib.request

from pymodbus.server import ModbusTcpServer
from pymodbus.simulator import DataType, SimData, SimDevice

COMMANDS = {"open": 0, "close": 1, "clamp": 2, "unclamp": 3, "start": 4}
STATES = {
    "door_open": 100,
    "door_closed": 101,
    "clamped": 102,
    "unclamped": 103,
    "idle": 104,
    "done": 105,
}

# The PLC program: each step's command, the state that ends it (and whether
# that state is to be on), and how long KYNEX's lathe takes over it.
STEPS = [
    ("open", "door_open", True, 3.0),
    ("close", "door_closed", True, 3.0),
    ("clamp", "clamped", True, 1.0),
    ("unclamp", "unclamped", True, 1.0),
]


def post(kynex, request):
    body = json.dumps(request).encode()
    req = urllib.request.Request(
        f"{kynex}/robot", body, {"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        answer = json.loads(r.read())
    if not answer.get("ok"):
        sys.exit(f"KYNEX refused {request}: {answer}")
    return answer


def event(kynex, name, timeout=10.0):
    """The latest `name` event from KYNEX's stream."""
    with urllib.request.urlopen(f"{kynex}/events", timeout=timeout) as r:
        start = time.monotonic()
        current = None
        while time.monotonic() - start < timeout:
            line = r.readline().decode().rstrip("\n")
            if line.startswith("event: "):
                current = line[7:]
            elif line.startswith("data: ") and current == name:
                return json.loads(line[6:])
    sys.exit(f"no {name} event from KYNEX")


def coil(words, n):
    return bool(words[n // 16] & (1 << (n % 16)))


def set_coil(words, n, on):
    if on:
        words[n // 16] |= 1 << (n % 16)
    else:
        words[n // 16] &= ~(1 << (n % 16))


class Program:
    """The PLC program, run once each time KYNEX asks the PLC anything."""

    def __init__(self, cycles):
        self.cycles = cycles
        self.step = 0
        self.cycle = 0
        self.started = None
        self.timings = []
        self.done = asyncio.Event()

    async def scan(self, _fc, _x, _address, _count, words, _values):
        if self.done.is_set():
            return None
        name, state, want, _ = STEPS[self.step]
        now = time.monotonic()
        if self.started is None:
            # Waiting for KYNEX to say the door is shut before the first step.
            if not coil(words, STATES["door_closed"]):
                return None
            self.started = now
        set_coil(words, COMMANDS[name], True)
        if coil(words, STATES[state]) == want:
            set_coil(words, COMMANDS[name], False)
            self.timings.append((self.cycle, name, now - self.started))
            self.started = now
            self.step += 1
            if self.step == len(STEPS):
                self.step = 0
                self.cycle += 1
                if self.cycle == self.cycles:
                    self.done.set()
        return None


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--kynex", default="http://127.0.0.1:8097")
    parser.add_argument("--port", type=int, default=5020)
    parser.add_argument("--cycles", type=int, default=2)
    args = parser.parse_args()

    program = Program(args.cycles)
    words = [SimData(0, count=16, values=0, datatype=DataType.REGISTERS)]
    device = SimDevice(
        1,
        simdata=(
            [SimData(0, count=1024, values=False, datatype=DataType.BITS)],
            [SimData(0, count=1, values=False, datatype=DataType.BITS)],
            words,
            words,
        ),
        action=program.scan,
    )
    server = ModbusTcpServer(device, address=("127.0.0.1", args.port))
    serving = asyncio.create_task(server.serve_forever())
    await asyncio.sleep(0.3)

    # A lathe, wired to the PLC.
    post(args.kynex, {"op": "place", "template": "cnc_lathe", "at": [0, 0, 0]})
    await asyncio.sleep(0.5)
    library = event(args.kynex, "library")
    lathe = [i for i in library["items"] if i["template"] == "cnc_lathe"][-1]
    machine = lathe["root"]
    for key, n in {**COMMANDS, **STATES}.items():
        post(args.kynex, {"op": "plc", "object": machine, "key": key, "coil": n})
    post(args.kynex, {"op": "plc_link", "host": "127.0.0.1", "port": args.port, "unit": 1, "on": True})
    print(f"{lathe['name']} wired to the PLC on port {args.port}")

    try:
        await asyncio.wait_for(program.done.wait(), timeout=30 * args.cycles)
    except asyncio.TimeoutError:
        print("the PLC program did not finish:", program.timings)
        return 1
    finally:
        post(args.kynex, {"op": "plc_link", "host": "127.0.0.1", "port": args.port, "unit": 1, "on": False})
        await server.shutdown()
        serving.cancel()

    worst = 0.0
    for cycle, name, took in program.timings:
        want = next(s[3] for s in STEPS if s[0] == name)
        worst = max(worst, abs(took - want))
        print(f"cycle {cycle + 1}  {name:8s} {took:5.2f} s  (lathe: {want:.1f} s)")
    print(f"worst difference: {worst * 1000:.0f} ms")
    return 0 if worst < 0.3 else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
