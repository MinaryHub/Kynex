"""A machine followed over umati (OPC UA for Machine Tools), end to end: an
OPC UA server here loads the OPC Foundation's own nodesets — DI, IA,
Machinery, ISA95 Job Control, MachineTool, fetched from OPCFoundation/
UA-Nodeset into target/umati — and makes a machine tool of MachineToolType
from them, whose state machines a routine drives: idle, then executing its
program, then the program ended (a part made), over and over. A running
KYNEX follows it by the specifications' browse names alone, and what
KYNEX's lathe did is read back from its stream.

Needs asyncua on a Python it works on (1.1.5 on Python 3.8 here; see the
README's PLC section):

    kynex view --empty --port 8101 --project target/umati/kynex-project.json
    target/plc/venv38/Scripts/python tools/umati/check.py
"""

import argparse
import asyncio
import json
import logging
import pathlib
import sys
import time
import urllib.request

from asyncua import Server, ua
from asyncua.common import xmlimporter

logging.basicConfig(level=logging.ERROR)

NODESETS = [
    ("DI/Opc.Ua.Di.NodeSet2.xml", "Opc.Ua.Di.NodeSet2.xml"),
    ("IA/Opc.Ua.IA.NodeSet2.xml", "Opc.Ua.IA.NodeSet2.xml"),
    ("Machinery/Opc.Ua.Machinery.NodeSet2.xml", "Opc.Ua.Machinery.NodeSet2.xml"),
    ("ISA95-JOBCONTROL/opc.ua.isa95-jobcontrol.nodeset2.xml", "Opc.Ua.ISA95-JOBCONTROL.NodeSet2.xml"),
    ("MachineTool/Opc.Ua.MachineTool.NodeSet2.xml", "Opc.Ua.MachineTool.NodeSet2.xml"),
]
BASE = "https://raw.githubusercontent.com/OPCFoundation/UA-Nodeset/latest/"

# The machine's routine: its item state, its program's state, how long.
ROUTINE = [
    ("NotExecuting", "Initializing", 1.5),
    ("Executing", "Running", 4.0),
    ("NotExecuting", "Ended", 1.5),
]


async def _no_check(self, *args, **kwargs):
    # The nodesets ask for a newer base model than this asyncua carries;
    # their types load all the same.
    return None


xmlimporter.XmlImporter._check_required_models = _no_check


def post(kynex, request):
    body = json.dumps(request).encode()
    req = urllib.request.Request(f"{kynex}/robot", body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=10) as r:
        answer = json.loads(r.read())
    if not answer.get("ok"):
        sys.exit(f"KYNEX refused {request}: {answer}")
    return answer


def events(kynex, name):
    with urllib.request.urlopen(f"{kynex}/events", timeout=30) as r:
        current = None
        for raw in r:
            line = raw.decode().rstrip("\n")
            if line.startswith("event: "):
                current = line[7:]
            elif line.startswith("data: ") and current == name:
                yield json.loads(line[6:])


async def machine_tool(port, folder):
    folder.mkdir(parents=True, exist_ok=True)
    for remote, local in NODESETS:
        path = folder / local
        if not path.exists():
            urllib.request.urlretrieve(BASE + remote, path)
    server = Server()
    await server.init()
    server.set_endpoint(f"opc.tcp://127.0.0.1:{port}/umati/")
    server.set_security_policy([ua.SecurityPolicyType.NoSecurity])
    for _, local in NODESETS:
        await server.import_xml(str(folder / local), strict_mode=False)
    own = await server.register_namespace("urn:kynex:check:umati")
    mt = await server.get_namespace_index("http://opcfoundation.org/UA/MachineTool/")
    machinery = await server.get_namespace_index("http://opcfoundation.org/UA/Machinery/")
    machines = await server.nodes.objects.get_child(f"{machinery}:Machines")
    lathe = await machines.add_object(
        ua.NodeId("Lathe1", own), ua.QualifiedName("KYNEX-Lathe", own), objecttype=ua.NodeId(13, mt)
    )
    item = await lathe.get_child([f"{mt}:Monitoring", f"{mt}:MachineTool", f"{machinery}:MachineryItemState", "0:CurrentState"])
    program = await lathe.get_child([f"{mt}:Production", f"{mt}:ActiveProgram", f"{mt}:State", "0:CurrentState"])
    name = await lathe.get_child([f"{mt}:Production", f"{mt}:ActiveProgram", f"{mt}:Name"])
    await name.write_value(ua.Variant("O1234", ua.VariantType.String))
    await server.start()

    async def run():
        while True:
            for item_state, program_state, length in ROUTINE:
                await item.write_value(ua.LocalizedText(item_state))
                await program.write_value(ua.LocalizedText(program_state))
                await asyncio.sleep(length)

    task = asyncio.create_task(run())

    async def stop():
        task.cancel()
        await server.stop()

    return stop, f"ns={own};s=Lathe1"


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--kynex", default="http://127.0.0.1:8101")
    parser.add_argument("--port", type=int, default=4844)
    parser.add_argument("--seconds", type=float, default=22.0)
    args = parser.parse_args()

    stop, node = await machine_tool(args.port, pathlib.Path("target/umati"))
    print(f"machine tool {node} on opc.tcp://127.0.0.1:{args.port}/umati/")
    post(args.kynex, {"op": "place", "template": "cnc_lathe", "at": [0, 0, 0]})
    await asyncio.sleep(0.5)
    library = next(events(args.kynex, "library"))
    machine = [i for i in library["items"] if i["template"] == "cnc_lathe"][-1]["root"]
    post(args.kynex, {
        "op": "mirror", "object": machine, "kind": "umati",
        "url": f"opc.tcp://127.0.0.1:{args.port}/umati/", "node": node, "security": "none",
    })

    # The stream read away from the server's loop.
    seen = []
    start = time.monotonic()

    def watch():
        for cell in events(args.kynex, "cell"):
            c = next((c for c in cell["components"] if c["object"] == machine), None)
            m = (c or {}).get("machine") or {}
            state = ("가공" if m.get("progress") is not None else "대기", m.get("cycles", 0))
            if not seen or seen[-1][1] != state:
                last = ((m.get("mirror") or {}).get("last") or {})
                seen.append((time.monotonic() - start, state))
                print(f"{seen[-1][0]:5.1f} s  {state[0]}  사이클 {state[1]}  "
                      f"({last.get('execution')}, {last.get('program')}, {(m.get('mirror') or {}).get('error')})", flush=True)
            if time.monotonic() - start > args.seconds:
                return

    await asyncio.get_running_loop().run_in_executor(None, watch)
    post(args.kynex, {"op": "mirror", "object": machine, "agent": ""})
    await stop()
    states = [s for _, s in seen]
    ok = any(s[0] == "가공" for s in states) and max(s[1] for s in states) >= 2
    print("ok" if ok else "missed part of the routine")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
