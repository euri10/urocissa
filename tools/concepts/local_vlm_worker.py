#!/usr/bin/env python3
"""Local, owner-triggered image-to-candidate worker; no model downloads."""
import argparse
from contextlib import contextmanager
import errno
import fcntl
import io
import json
import os
from pathlib import Path
import select
import signal
import socket
import socketserver
import stat
import struct
import time

MAX_IMAGE = 8 * 1024 * 1024
MAX_REPLY = 8192
INFERENCE_SECONDS = 90
PROMPT = "What objects and surroundings are visible in this photo? Answer with a short comma-separated list."
# Only reviewed labels and exact aliases can be model suggestions. User-entered
# tags use a different path and are not restricted by this vocabulary.
VOCABULARY = Path(__file__).resolve().parents[2] / "gallery-backend/src/operations/concept_candidates/allowed_labels.txt"
ALIASES = {}
for line in VOCABULARY.read_text(encoding="utf-8").splitlines():
    canonical, *aliases = line.split("|")
    for alias in [canonical, *aliases]:
        if alias in ALIASES:
            raise ValueError("duplicate concept vocabulary alias")
        ALIASES[alias] = canonical


def candidates(output):
    """Return only exact reviewed labels; never extract words from model prose."""
    if not isinstance(output, str) or len(output) > MAX_REPLY or "\n" in output:
        return []
    result = []
    for raw in output.strip().rstrip(".").split(","):
        label = ALIASES.get(" ".join(raw.strip().split()).lower())
        if label is None:
            continue
        if label not in result:
            result.append(label)
        if len(result) == 8:
            break
    return result


def receive(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.recv(size - len(data))
        if not chunk:
            raise ValueError("incomplete request")
        data.extend(chunk)
    return bytes(data)


def configure_processor(processor):
    # ponytail: one overview patch bounds CPU latency; fine detail may be missed.
    processor.image_processor.do_image_splitting = False
    return processor


class InferenceGuard:
    """Stop between generated tokens when the deadline or caller disappears."""
    def __init__(self, connection, seconds=INFERENCE_SECONDS):
        self.connection = connection
        self.deadline = time.monotonic() + seconds

    def __call__(self, _input_ids, _scores, **_kwargs):
        if time.monotonic() >= self.deadline:
            return True
        try:
            # After the complete request, readable means EOF or invalid extra input.
            return bool(select.select([self.connection], [], [], 0)[0])
        except (OSError, ValueError):
            return True


def generate_checked(model, inputs, guard, inference_mode):
    if guard(None, None):
        raise TimeoutError("request no longer active")
    with inference_mode():
        generated = model.generate(**inputs, max_new_tokens=96, do_sample=False, stopping_criteria=[guard])
    if guard(None, None):
        raise TimeoutError("request no longer active")
    return generated


@contextmanager
def socket_lease(socket_path):
    """Serialize workers and remove only a verified stale socket in a trusted directory."""
    lock_path = socket_path.with_name(socket_path.name + ".lock")
    fd = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "r+") as lock:
        info = os.fstat(lock.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid():
            raise ValueError("invalid socket lock")
        os.fchmod(lock.fileno(), 0o600)
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            info = socket_path.lstat()
        except FileNotFoundError:
            info = None
        if info is not None:
            if not stat.S_ISSOCK(info.st_mode) or info.st_uid != os.geteuid():
                raise ValueError("refusing to replace an untrusted socket path")
            with socket.socket(socket.AF_UNIX) as probe:
                probe.settimeout(1)
                if probe.connect_ex(str(socket_path)) != errno.ECONNREFUSED:
                    raise ValueError("socket is in use or cannot be checked")
            socket_path.unlink()
        yield


def serve(model_dir, socket_path):
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    import torch
    from PIL import Image
    from transformers import AutoModelForVision2Seq, AutoProcessor

    torch.set_num_threads(4)
    Image.MAX_IMAGE_PIXELS = 25_000_000
    processor = configure_processor(AutoProcessor.from_pretrained(model_dir, local_files_only=True, trust_remote_code=False))
    model = AutoModelForVision2Seq.from_pretrained(model_dir, local_files_only=True, trust_remote_code=False).eval()
    messages = [{"role": "user", "content": [{"type": "image"}, {"type": "text", "text": PROMPT}]}]
    prompt = processor.apply_chat_template(messages, add_generation_prompt=True)

    class Handler(socketserver.BaseRequestHandler):
        def handle(self):
            self.request.settimeout(120)
            guard = InferenceGuard(self.request)
            try:
                size = struct.unpack("!I", receive(self.request, 4))[0]
                if not 0 < size <= MAX_IMAGE:
                    raise ValueError("invalid image size")
                with Image.open(io.BytesIO(receive(self.request, size))) as source:
                    if source.format != "JPEG":
                        raise ValueError("expected JPEG thumbnail")
                    if source.width * source.height > Image.MAX_IMAGE_PIXELS:
                        raise ValueError("thumbnail dimensions exceed limit")
                    image = source.convert("RGB")
                if guard(None, None):
                    raise TimeoutError("request no longer active")
                inputs = processor(text=prompt, images=[image], return_tensors="pt")
                generated = generate_checked(model, inputs, guard, torch.inference_mode)
                output = processor.decode(generated[0][inputs["input_ids"].shape[-1]:], skip_special_tokens=True)
                reply = {"candidates": candidates(output)}
            except Exception:  # Do not log image bytes, paths, model output or metadata.
                reply = {"error": "Could not analyze the photo."}
            encoded = json.dumps(reply).encode("utf-8")
            try:
                self.request.sendall(struct.pack("!I", len(encoded)) + encoded)
            except OSError:
                pass  # The caller may have timed out; no image data should be logged.

    # ponytail: serialize requests for this small optional worker; queueing is bounded by timeouts.
    old_umask = os.umask(0o077)
    try:
        with socket_lease(socket_path), socketserver.UnixStreamServer(str(socket_path), Handler) as server:
            socket_id = socket_path.stat().st_ino
            os.umask(old_umask)
            print("Local concept worker ready", flush=True)
            try:
                server.serve_forever()
            finally:
                if socket_path.exists() and socket_path.stat().st_ino == socket_id:
                    socket_path.unlink()
    finally:
        os.umask(old_umask)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--socket", type=Path, required=True)
    args = parser.parse_args()
    def stop(_signum, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop)
    try:
        serve(args.model_dir.resolve(strict=True), args.socket.absolute())
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
