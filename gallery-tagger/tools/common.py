"""Shared helpers for gallery-tagger model tooling."""
import hashlib
import json
import pathlib
import shutil

TOOLS = pathlib.Path(__file__).resolve().parent
LOCK_PATH = TOOLS.parent / "model.lock.json"
TEXT_MAX_TOKENS = 64


def load_lock() -> dict:
    return json.loads(LOCK_PATH.read_text())


def variant_spec(variant: str | None) -> tuple[str, dict]:
    lock = load_lock()
    name = variant or lock["default_variant"]
    return name, lock["variants"][name]


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fetch_verified(spec: dict, source_dir: pathlib.Path | None, staging: pathlib.Path) -> pathlib.Path:
    """Copy (or download) every locked file into `staging` and verify its sha256."""
    staging.mkdir(parents=True, exist_ok=True)
    for name, expected in spec["files"].items():
        target = staging / name
        if source_dir is not None:
            shutil.copyfile(source_dir / name, target)
        else:
            from huggingface_hub import hf_hub_download

            downloaded = hf_hub_download(spec["repo"], name, revision=spec["revision"])
            shutil.copyfile(downloaded, target)
        actual = sha256(target)
        if actual != expected:
            raise SystemExit(f"sha256 mismatch for {name}: expected {expected}, got {actual}")
    return staging


def model_id(variant: str, spec: dict, precision: str) -> str:
    return f"{variant}@{spec['revision'][:12]}/{precision}"
