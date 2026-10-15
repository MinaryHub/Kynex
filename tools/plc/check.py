"""Virtual commissioning, checked against servers that are not KYNEX's own:
a free Modbus TCP server (pymodbus) or a free OPC UA server (asyncua) plays
a PLC, and a small PLC program in this script runs a CNC lathe in a running
KYNEX through its cycle.

    pip install pymodbus asyncua
    kynex view --empty --port 8097 --project target/plc/kynex-project.json
    python tools/plc/check.py                    # Modbus TCP
    python tools/plc/check.py --protocol opcua   # OPC UA

The script places a lathe in that KYNEX (over its page's requests), wires
the lathe's commands and states to the PLC — over Modbus, commands to coils
0-4 and states to coils 100-105; over OPC UA, Boolean variables
`ns=2;s=Lathe.<name>` under an object Lathe — links KYNEX to the PLC, and
runs its program: open the door, close it, clamp, unclamp, over and over.
Each step's command is turned on, held until the state says it is done,
then turned off, as a PLC program's outputs would be. It reports how long
each step took: the door takes the lathe's 3 s, the chuck its 1 s, give or
take KYNEX's 50 ms exchange and the program's own scan.
"""

import argparse
import asyncio
import json
import sys
import time
import urllib.request

COMMANDS = ["open", "close", "clamp", "unclamp", "start"]
STATES = ["door_open", "door_closed", "clamped", "unclamped", "idle", "done"]
# Over Modbus: each name's coil.
COILS = {
    **{name: i for i, name in enumerate(COMMANDS)},
    **{name: 100 + i for i, name in enumerate(STATES)},
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


class Program:
    """The PLC program: one scan reads the states, sets the commands."""

    def __init__(self, cycles):
        self.cycles = cycles
        self.step = 0
        self.cycle = 0
        self.started = None
        self.timings = []
        self.done = asyncio.Event()

    def scan(self, state, command):
        """`state(name)` reads a state; `command(name, on)` sets one."""
        if self.done.is_set():
            return
        name, ends, want, _ = STEPS[self.step]
        now = time.monotonic()
        if self.started is None:
            # Waiting for KYNEX to say the door is shut before the first step.
            if not state("door_closed"):
                return
            self.started = now
        command(name, True)
        if state(ends) == want:
            command(name, False)
            self.timings.append((self.cycle, name, now - self.started))
            self.started = now
            self.step += 1
            if self.step == len(STEPS):
                self.step = 0
                self.cycle += 1
                if self.cycle == self.cycles:
                    self.done.set()


async def modbus_plc(program, port):
    """A Modbus TCP server whose program scans each time KYNEX asks it
    anything. Returns its stop, and each name's address."""
    from pymodbus.server import ModbusTcpServer
    from pymodbus.simulator import DataType, SimData, SimDevice

    def coil(words, n):
        return bool(words[n // 16] & (1 << (n % 16)))

    def set_coil(words, n, on):
        if on:
            words[n // 16] |= 1 << (n % 16)
        else:
            words[n // 16] &= ~(1 << (n % 16))

    async def scan(_fc, _x, _address, _count, words, _values):
        program.scan(
            lambda name: coil(words, COILS[name]),
            lambda name, on: set_coil(words, COILS[name], on),
        )

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
    server = ModbusTcpServer(device, address=("127.0.0.1", port))
    serving = asyncio.create_task(server.serve_forever())
    await asyncio.sleep(0.3)

    async def stop():
        await server.shutdown()
        serving.cancel()

    link = {"protocol": "modbus", "host": "127.0.0.1", "port": port, "unit": 1}
    return stop, link, {name: str(n) for name, n in COILS.items()}


async def opcua_plc(program, port):
    """An OPC UA server with no security and anonymous access, its program
    scanning every 10 ms. Returns its stop, and each name's node id."""
    from asyncua import Server, ua

    server = Server()
    await server.init()
    server.set_endpoint(f"opc.tcp://127.0.0.1:{port}/kynex/")
    server.set_security_policy([ua.SecurityPolicyType.NoSecurity])
    ns = await server.register_namespace("urn:kynex:check")
    lathe = await server.nodes.objects.add_object(ua.NodeId("Lathe", ns), "Lathe")
    nodes = {}
    for name in COMMANDS + STATES:
        var = await lathe.add_variable(ua.NodeId(f"Lathe.{name}", ns), name, False)
        await var.set_writable()
        nodes[name] = var
    await server.start()
    values = {name: False for name in nodes}

    async def scanning():
        while True:
            for name in STATES:
                values[name] = bool(await nodes[name].read_value())
            wanted = {}
            program.scan(lambda name: values[name], wanted.__setitem__)
            for name, on in wanted.items():
                if values[name] != on:
                    await nodes[name].write_value(on)
                    values[name] = on
            await asyncio.sleep(0.01)

    task = asyncio.create_task(scanning())

    async def stop():
        task.cancel()
        await server.stop()

    link = {"protocol": "opcua", "host": "127.0.0.1", "port": port, "path": "/kynex/"}
    return stop, link, {name: f"ns={ns};s=Lathe.{name}" for name in nodes}


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--kynex", default="http://127.0.0.1:8097")
    parser.add_argument("--protocol", choices=["modbus", "opcua"], default="modbus")
    parser.add_argument("--port", type=int)
    parser.add_argument("--cycles", type=int, default=2)
    args = parser.parse_args()

    program = Program(args.cycles)
    if args.protocol == "modbus":
        stop, link, addresses = await modbus_plc(program, args.port or 5020)
    else:
        stop, link, addresses = await opcua_plc(program, args.port or 4840)

    # A lathe, wired to the PLC.
    post(args.kynex, {"op": "place", "template": "cnc_lathe", "at": [0, 0, 0]})
    await asyncio.sleep(0.5)
    library = event(args.kynex, "library")
    lathe = [i for i in library["items"] if i["template"] == "cnc_lathe"][-1]
    machine = lathe["root"]
    for key, address in addresses.items():
        post(args.kynex, {"op": "plc", "object": machine, "key": key, "address": address})
    post(args.kynex, {"op": "plc_link", **link, "on": True})
    print(f"{lathe['name']} wired to the PLC over {args.protocol}, port {link['port']}")

    try:
        await asyncio.wait_for(program.done.wait(), timeout=30 * args.cycles)
    except asyncio.TimeoutError:
        print("the PLC program did not finish:", program.timings)
        print("KYNEX says:", event(args.kynex, "plc"))
        return 1
    finally:
        post(args.kynex, {"op": "plc_link", **link, "on": False})
        await stop()

    worst = 0.0
    for cycle, name, took in program.timings:
        want = next(s[3] for s in STEPS if s[0] == name)
        worst = max(worst, abs(took - want))
        print(f"cycle {cycle + 1}  {name:8s} {took:5.2f} s  (lathe: {want:.1f} s)")
    print(f"worst difference: {worst * 1000:.0f} ms")
    return 0 if worst < 0.3 else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
