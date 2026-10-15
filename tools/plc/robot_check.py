"""A PLC starting a robot's program, over Modbus TCP: pymodbus (free) plays
the PLC, KYNEX's PLC signal table ties coil 10 to starting the robot's
program and coil 110 to it running, and the program itself sets coil 111
("준비") as it goes. Set up in the viewer first (see README, "PLC"):

    python tools/plc/robot_check.py --kynex http://127.0.0.1:8099

The script turns coil 10 on, and reports when KYNEX says the robot runs
(110), when its program says it is ready (111), and when it has finished
(110 off again).
"""

import argparse
import asyncio
import json
import sys
import time
import urllib.request

from pymodbus.server import ModbusTcpServer
from pymodbus.simulator import DataType, SimData, SimDevice


def post(kynex, request):
    body = json.dumps(request).encode()
    req = urllib.request.Request(f"{kynex}/robot", body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=10) as r:
        answer = json.loads(r.read())
    if not answer.get("ok"):
        sys.exit(f"KYNEX refused {request}: {answer}")
    return answer


def coil(words, n):
    return bool(words[n // 16] & (1 << (n % 16)))


def set_coil(words, n, on):
    if on:
        words[n // 16] |= 1 << (n % 16)
    else:
        words[n // 16] &= ~(1 << (n % 16))


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--kynex", default="http://127.0.0.1:8099")
    parser.add_argument("--port", type=int, default=5021)
    args = parser.parse_args()

    seen = {}
    start = [None]

    async def scan(_fc, _x, _address, _count, words, _values):
        now = time.monotonic()
        if start[0] is None:
            start[0] = now
            set_coil(words, 10, True)
        t = now - start[0]
        for name, n, on in [("running", 110, True), ("ready", 111, True), ("finished", 110, False)]:
            if name not in seen and coil(words, n) == on and (name != "finished" or "running" in seen):
                seen[name] = t
        # The start held for a second, as a PLC's pulse.
        if t > 1.0:
            set_coil(words, 10, False)

    registers = [SimData(0, count=16, values=0, datatype=DataType.REGISTERS)]
    device = SimDevice(
        1,
        simdata=(
            [SimData(0, count=1024, values=False, datatype=DataType.BITS)],
            [SimData(0, count=1, values=False, datatype=DataType.BITS)],
            registers,
            registers,
        ),
        action=scan,
    )
    server = ModbusTcpServer(device, address=("127.0.0.1", args.port))
    serving = asyncio.create_task(server.serve_forever())
    await asyncio.sleep(0.3)
    link = {"protocol": "modbus", "host": "127.0.0.1", "port": args.port, "unit": 1}
    post(args.kynex, {"op": "plc_link", **link, "on": True})
    deadline = time.monotonic() + 60
    while "finished" not in seen and time.monotonic() < deadline:
        await asyncio.sleep(0.1)
    post(args.kynex, {"op": "plc_link", **link, "on": False})
    await server.shutdown()
    serving.cancel()
    for name in ["running", "ready", "finished"]:
        print(f"{name:9s} {seen.get(name, float('nan')):6.2f} s after the start coil")
    return 0 if len(seen) == 3 else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
