#!/usr/bin/env python3
"""Networkless local Florence worker; only mapped labels cross its socket."""
import argparse
from contextlib import contextmanager
import errno
import fcntl
import json
import os
from pathlib import Path
import resource
import select
import signal
import socket
import socketserver
import stat
import struct
import subprocess
import time

from mapping import candidates

MAX_IMAGE = 8 * 1024 * 1024
MAX_RAW = 1024 * 1024
DEADLINE = 600


def read_exact(fd, size, deadline):
    parts = bytearray()
    while len(parts) < size:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([fd], [], [], remaining)[0]:
            raise TimeoutError
        part = os.read(fd, size - len(parts))
        if not part:
            raise EOFError
        parts.extend(part)
    return bytes(parts)


def write_all(fd, data, deadline):
    view = memoryview(data)
    while view:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([], [fd], [], remaining)[1]:
            raise TimeoutError
        try:
            sent = os.write(fd, view)
        except BlockingIOError:
            continue
        if sent <= 0:
            raise EOFError
        view = view[sent:]


class Inference:
    def __init__(self):
        self.process = None

    def close(self):
        if self.process is not None:
            self.process.kill()
            self.process.wait()
            self.process.stdin.close()
            self.process.stdout.close()
            self.process = None

    def read_reply(self, deadline):
        fd = self.process.stdout.fileno()
        length = struct.unpack('!I', read_exact(fd, 4, deadline))[0]
        if not 0 < length <= MAX_RAW:
            raise ValueError('invalid inference reply')
        return json.loads(read_exact(fd, length, deadline))

    def start(self, deadline):
        self.process = subprocess.Popen(
            ['node', str(Path(__file__).with_name('infer.mjs'))],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            env={'PATH': '/opt/runtime:/usr/local/bin:/usr/bin:/bin', 'NODE_NO_WARNINGS': '1'},
        )
        os.set_blocking(self.process.stdin.fileno(), False)
        if self.read_reply(deadline) != {'ready': True}:
            raise ValueError('inference unavailable')

    def analyze(self, image):
        deadline = time.monotonic() + DEADLINE
        try:
            if self.process is None:
                self.start(deadline)
            write_all(self.process.stdin.fileno(), struct.pack('!I', len(image)) + image, deadline)
            reply = self.read_reply(deadline)
            od, dense, caption = reply['od'], reply['dense'], reply['caption']
            if (not isinstance(od, list) or not isinstance(dense, list)
                    or not all(isinstance(value, str) for value in od + dense)
                    or not isinstance(caption, str)):
                raise ValueError('invalid inference reply')
            return candidates(od, dense, caption)
        except Exception:
            self.close()
            raise


@contextmanager
def socket_lease(path):
    fd = os.open(path.with_name(path.name + '.lock'), os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'r+') as lock:
        info = os.fstat(lock.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid():
            raise ValueError('invalid socket lock')
        os.fchmod(lock.fileno(), 0o600)
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            info = path.lstat()
        except FileNotFoundError:
            info = None
        if info is not None:
            if not stat.S_ISSOCK(info.st_mode) or info.st_uid != os.geteuid():
                raise ValueError('invalid socket path')
            with socket.socket(socket.AF_UNIX) as probe:
                probe.settimeout(1)
                if probe.connect_ex(str(path)) != errno.ECONNREFUSED:
                    raise ValueError('socket unavailable')
            path.unlink()
        yield


def receive(stream, size):
    data = bytearray()
    while len(data) < size:
        part = stream.recv(size - len(data))
        if not part:
            raise EOFError
        data.extend(part)
    return bytes(data)


def serve(path):
    inference = Inference()

    class Handler(socketserver.BaseRequestHandler):
        def handle(self):
            self.request.settimeout(DEADLINE)
            try:
                size = struct.unpack('!I', receive(self.request, 4))[0]
                if not 0 < size <= MAX_IMAGE:
                    raise ValueError('invalid image')
                labels = inference.analyze(receive(self.request, size))
                reply = {'candidates': labels}
            except Exception:
                reply = {'error': 'Could not analyze the photo.'}
            encoded = json.dumps(reply).encode('utf-8')
            try:
                self.request.sendall(struct.pack('!I', len(encoded)) + encoded)
            except OSError:
                pass

    class Server(socketserver.UnixStreamServer):
        def handle_error(self, _request, _client_address):
            pass

    old_umask = os.umask(0o077)
    try:
        with socket_lease(path):
            inference.start(time.monotonic() + DEADLINE)
            with Server(str(path), Handler) as server:
                inode = path.stat().st_ino
                os.umask(old_umask)
                try:
                    server.serve_forever()
                finally:
                    if path.exists() and path.stat().st_ino == inode:
                        path.unlink()
    finally:
        inference.close()
        os.umask(old_umask)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--socket', type=Path, required=True)
    args = parser.parse_args()
    # Native runtime diagnostics can contain tensors. Never log any worker output.
    with open(os.devnull, 'w', encoding='utf-8') as sink:
        os.dup2(sink.fileno(), 1)
        os.dup2(sink.fileno(), 2)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.nice(15)
    def stop(_signum, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop)
    try:
        serve(args.socket.absolute())
    except KeyboardInterrupt:
        pass
    except Exception:
        raise SystemExit(1) from None


if __name__ == '__main__':
    main()
