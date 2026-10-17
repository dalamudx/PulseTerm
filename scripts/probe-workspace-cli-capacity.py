"""Actual admitted UDS slots; never retain token, wire payloads or credential data."""
import json
from pathlib import Path
import socket
import struct
import subprocess
import time


def read_frame(stream):
    def exact(count):
        chunks = bytearray()
        while len(chunks) < count:
            part = stream.recv(count - len(chunks))
            assert part, "cli.capacity_unexpected_eof"
            chunks.extend(part)
        return chunks
    length = struct.unpack("<I", exact(4))[0]
    assert 0 < length <= 65536
    return json.loads(exact(length))


def verify(pulse, env, runtime, evidence):
    slots = []
    secret = bytearray((runtime / "control.token").read_bytes())
    assert len(secret) == 64
    body = bytearray(json.dumps({"jsonrpc": "2.0", "id": "capacity", "method": "hello", "params": {"protocolVersion": 1, "token": secret.decode("ascii")}}, separators=(",", ":")).encode())
    original = tuple((path.stat().st_dev, path.stat().st_ino, path.stat().st_mode) for path in [runtime / "control.sock", runtime / "control.token"])
    try:
        def hello():
            stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            stream.settimeout(2)
            try:
                stream.connect(str(runtime / "control.sock"))
                stream.sendall(struct.pack("<I", len(body)))
                stream.sendall(body)
                reply = read_frame(stream)
                assert reply == {"jsonrpc": "2.0", "id": "capacity", "result": {"protocolVersion": 1}}, "cli.capacity_hello_not_admitted"
                return stream
            except BaseException:
                stream.close()
                raise
        started = time.monotonic()
        # Each authenticated reply proves the server task, not just kernel backlog, owns a slot.
        for _ in range(8):
            slots.append(hello())
        overflow = subprocess.run([str(pulse), "-w", "workspace", "focus"], env=env, capture_output=True, text=True, timeout=2)
        assert overflow.returncode != 0 and not overflow.stdout, "cli.capacity_ninth_client_admitted"
        assert time.monotonic() - started < 4, "cli.capacity_handshake_slots_expired"
        slots.pop().close()
        # Wait only for read-only hello admission. No action is retried or spawned.
        end = time.monotonic() + 1
        while True:
            try:
                recovered = hello()
                break
            except (AssertionError, OSError):
                assert time.monotonic() < end, "cli.capacity_slot_not_reclaimed"
                time.sleep(0.01)
        recovered.close()
        assert tuple((path.stat().st_dev, path.stat().st_ino, path.stat().st_mode) for path in [runtime / "control.sock", runtime / "control.token"]) == original
        (evidence / "capacity-result.json").write_text(json.dumps({"authenticated_slots": 8, "ninth_cli_rejected": True, "closed_slot_reclaimed": True, "endpoint_identity_retained": True}) + "\n")
    finally:
        for stream in slots:
            stream.close()
        for value in [secret, body]:
            value[:] = b"\0" * len(value)
