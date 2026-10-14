"""A small RTDE client (Universal Robots' real-time data exchange, port
30004): enough to read the controller's state as it runs, with the
standard library alone.

Only outputs are used: the controller's clock, the joint positions and an
integer register a script writes, sampled at up to 500 Hz.
"""

import socket
import struct

PORT = 30004

# Package types.
REQUEST_PROTOCOL_VERSION = ord('V')
SETUP_OUTPUTS = ord('O')
START = ord('S')
PAUSE = ord('P')
DATA = ord('U')

# The bytes each type a recipe names takes, and how it unpacks.
TYPES = {
    'DOUBLE': 'd',
    'VECTOR6D': '6d',
    'VECTOR3D': '3d',
    'INT32': 'i',
    'UINT32': 'I',
    'UINT64': 'Q',
    'UINT8': 'B',
    'BOOL': '?',
}


class Rtde:
    def __init__(self, host='127.0.0.1', port=PORT, timeout=5.0):
        self.sock = socket.create_connection((host, port), timeout)
        self.buf = b''
        self.names = []
        self.format = ''

    def send(self, kind, payload=b''):
        self.sock.sendall(struct.pack('>HB', len(payload) + 3, kind) + payload)

    def receive(self):
        """The next package: its type and payload."""
        while True:
            if len(self.buf) >= 3:
                size, kind = struct.unpack_from('>HB', self.buf)
                if len(self.buf) >= size:
                    payload = self.buf[3:size]
                    self.buf = self.buf[size:]
                    return kind, payload
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError('RTDE closed')
            self.buf += chunk

    def answer(self, kind):
        while True:
            k, payload = self.receive()
            if k == kind:
                return payload

    def setup(self, names, frequency=500.0):
        self.send(REQUEST_PROTOCOL_VERSION, struct.pack('>H', 2))
        if not self.answer(REQUEST_PROTOCOL_VERSION)[0]:
            raise RuntimeError('RTDE protocol 2 refused')
        self.send(SETUP_OUTPUTS, struct.pack('>d', frequency) + ','.join(names).encode())
        payload = self.answer(SETUP_OUTPUTS)
        types = payload[1:].decode().split(',')
        if 'NOT_FOUND' in types:
            raise RuntimeError(f'unknown outputs: {list(zip(names, types))}')
        self.names = names
        self.format = '>' + ''.join(TYPES[t] for t in types)
        self.types = types

    def start(self):
        self.send(START)
        if not self.answer(START)[0]:
            raise RuntimeError('RTDE start refused')

    def sample(self):
        """The next sample, as a dict by name."""
        payload = self.answer(DATA)
        values = struct.unpack_from(self.format, payload, 1)
        out, i = {}, 0
        for name, t in zip(self.names, self.types):
            n = {'VECTOR6D': 6, 'VECTOR3D': 3}.get(t, 1)
            out[name] = values[i] if n == 1 else list(values[i:i + n])
            i += n
        return out

    def close(self):
        try:
            self.send(PAUSE)
        except OSError:
            pass
        self.sock.close()
