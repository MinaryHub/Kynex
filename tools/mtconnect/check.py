"""A machine followed over MTConnect, end to end: a small agent here serves a
device's current state in MTConnect Streams' form (as the MTConnect
Institute's agent writes it), its machine opening its door, closing it,
cutting and making a part, over and over; a running KYNEX follows it with
a lathe, and what KYNEX's lathe did is read back from its stream.

    kynex view --empty --port 8100 --project target/mtc/kynex-project.json
    python tools/mtconnect/check.py

`--agent http://demo.mtconnect.org:5000 --device Mazak` follows the
Institute's public demo agent instead, and reports what KYNEX shows.
"""

import argparse
import http.server
import json
import sys
import threading
import time
import urllib.request

# The machine's routine: what it reports, and for how long.
ROUTINE = [
    ("OPEN", "READY", 0, 2.0),
    ("CLOSED", "READY", 0, 1.0),
    ("CLOSED", "ACTIVE", 0, 4.0),
    ("CLOSED", "READY", 1, 1.0),
]

STREAMS = """<?xml version="1.0" encoding="UTF-8"?>
<MTConnectStreams xmlns="urn:mtconnect.org:MTConnectStreams:2.8"><Header creationTime="{now}" sender="kynex-check" instanceId="1" version="2.8.0" bufferSize="131072" nextSequence="{seq}" firstSequence="1" lastSequence="{seq}"/><Streams><DeviceStream name="Agent" uuid="agent"><ComponentStream component="Agent" name="agent" componentId="agent"><Events><Availability dataItemId="agent_avail" sequence="1" timestamp="{now}">AVAILABLE</Availability></Events></ComponentStream></DeviceStream><DeviceStream name="Lathe1" uuid="LATHE-1"><ComponentStream component="Device" name="Lathe1" componentId="d"><Events><Availability dataItemId="avail" sequence="2" timestamp="{now}">AVAILABLE</Availability></Events></ComponentStream><ComponentStream component="Door" name="door" componentId="door"><Events><DoorState dataItemId="doorstate" sequence="{seq}" timestamp="{now}">{door}</DoorState></Events></ComponentStream><ComponentStream component="Path" name="path" componentId="path"><Events><Execution dataItemId="execution" sequence="{seq}" timestamp="{now}">{execution}</Execution><PartCount dataItemId="partcount" sequence="{seq}" timestamp="{now}">{parts}</PartCount><Program dataItemId="program" sequence="3" timestamp="{now}">O1234</Program></Events></ComponentStream></DeviceStream></Streams></MTConnectStreams>"""


class Machine:
    def __init__(self):
        self.start = time.monotonic()
        self.cycle = sum(step[3] for step in ROUTINE)

    def state(self):
        t = time.monotonic() - self.start
        done, into = divmod(t, self.cycle)
        for door, execution, more, length in ROUTINE:
            if into < length:
                return door, execution, int(done) + more
            into -= length
        return ROUTINE[-1][0], ROUTINE[-1][1], int(done) + 1


def serve(port, machine):
    class Agent(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            door, execution, parts = machine.state()
            if not self.path.endswith("/current"):
                self.send_error(404)
                return
            body = STREAMS.format(
                now=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                seq=int(time.monotonic() * 10),
                door=door,
                execution=execution,
                parts=parts,
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/xml")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Agent)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def post(kynex, request):
    body = json.dumps(request).encode()
    req = urllib.request.Request(f"{kynex}/robot", body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=10) as r:
        answer = json.loads(r.read())
    if not answer.get("ok"):
        sys.exit(f"KYNEX refused {request}: {answer}")
    return answer


def events(kynex, name):
    """Every `name` event from KYNEX's stream, as it comes."""
    with urllib.request.urlopen(f"{kynex}/events", timeout=30) as r:
        current = None
        for raw in r:
            line = raw.decode().rstrip("\n")
            if line.startswith("event: "):
                current = line[7:]
            elif line.startswith("data: ") and current == name:
                yield json.loads(line[6:])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--kynex", default="http://127.0.0.1:8100")
    parser.add_argument("--port", type=int, default=5001)
    parser.add_argument("--agent")
    parser.add_argument("--device", default="Lathe1")
    parser.add_argument("--seconds", type=float, default=20.0)
    args = parser.parse_args()

    if args.agent is None:
        serve(args.port, Machine())
        agent = f"http://127.0.0.1:{args.port}"
    else:
        agent = args.agent
    post(args.kynex, {"op": "place", "template": "cnc_lathe", "at": [0, 0, 0]})
    time.sleep(0.5)
    library = next(events(args.kynex, "library"))
    machine = [i for i in library["items"] if i["template"] == "cnc_lathe"][-1]["root"]
    post(args.kynex, {"op": "mirror", "object": machine, "agent": agent, "device": args.device})
    print(f"following {args.device} at {agent}")

    start = time.monotonic()
    seen = []
    for cell in events(args.kynex, "cell"):
        c = next((c for c in cell["components"] if c["object"] == machine), None)
        m = (c or {}).get("machine") or {}
        state = (
            "열림" if m.get("door", 0) >= 1 else "닫힘",
            "가공" if m.get("progress") is not None else "대기",
            m.get("cycles", 0),
        )
        if not seen or seen[-1][1] != state:
            seen.append((time.monotonic() - start, state))
            mirror = m.get("mirror") or {}
            print(f"{seen[-1][0]:5.1f} s  문 {state[0]}  {state[1]}  사이클 {state[2]}  "
                  f"({(mirror.get('last') or {}).get('execution')}, {mirror.get('error')})")
        if time.monotonic() - start > args.seconds:
            break
    post(args.kynex, {"op": "mirror", "object": machine, "agent": ""})
    if args.agent is None:
        # The routine seen through: open, shut, cutting, a part made.
        states = [s for _, s in seen]
        ok = any(s[0] == "열림" for s in states) and any(s[1] == "가공" for s in states) and max(s[2] for s in states) >= 1
        print("ok" if ok else "missed part of the routine")
        return 0 if ok else 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
