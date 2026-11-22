#!/usr/bin/env python3
"""Opt-in local image test; only the repository's public cat fixture is sent."""
import json
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
IMAGE = "registry.gitlab.bartab.fr/infra/urocissa/concept-worker@sha256:18f39cf8a771989ef13cfbf9f2bf52e7bd9b06e47737d714243d7caa76e2fdbc"


def docker(*args, check=True):
    return subprocess.run(["docker", *args], check=check, capture_output=True, text=True, timeout=30)


def receive(stream, length):
    data = bytearray()
    while len(data) < length:
        chunk = stream.recv(length - len(data))
        if not chunk:
            raise RuntimeError("incomplete reply")
        data.extend(chunk)
    return bytes(data)


def connect(path, image):
    stream = socket.socket(socket.AF_UNIX)
    stream.settimeout(30)
    stream.connect(str(path))
    stream.sendall(struct.pack("!I", len(image)) + image)
    return stream


def main():
    name = "concept-test-" + uuid.uuid4().hex[:12]
    with tempfile.TemporaryDirectory(prefix="concept-test-") as directory:
        socket_path = Path(directory) / "worker.sock"
        fixture = ROOT / "gallery-tagger/tests/fixtures/images/cat.jpg"
        image = fixture.read_bytes()
        try:
            docker("run", "-d", "--name", name, "--network", "none", "--read-only", "--user", "1000:1000",
                   "--cpus", "2", "--memory", "4g", "--pids-limit", "128", "--cap-drop", "ALL",
                   "--security-opt", "no-new-privileges:true", "--tmpfs", "/tmp:size=128m,mode=1777,noexec,nosuid",
                   "--mount", f"type=bind,src={directory},dst=/socket",
                   "--mount", f"type=bind,src={ROOT / 'tools/concepts/local_vlm_worker.py'},dst=/app/tools/concepts/local_vlm_worker.py,readonly", IMAGE)
            deadline = time.monotonic() + 90
            while not socket_path.exists():
                if time.monotonic() >= deadline:
                    raise RuntimeError("worker failed to become ready")
                time.sleep(1)
            abandoned = connect(socket_path, image)
            time.sleep(2)
            abandoned.close()
            start = time.monotonic()
            with connect(socket_path, image) as client:
                size = struct.unpack("!I", receive(client, 4))[0]
                reply = json.loads(receive(client, size))
            elapsed = time.monotonic() - start
            if "cat" not in reply.get("candidates", []):
                raise AssertionError("expected cat in the public-fixture response")
            if elapsed >= 20:
                raise AssertionError("abandoned request delayed the next fixture for 20 seconds or more")
            print(json.dumps({"result": "pass", "labels": reply["candidates"], "next_request_seconds": round(elapsed, 3)}))
        finally:
            docker("rm", "-f", name, check=False)


if __name__ == "__main__":
    main()
