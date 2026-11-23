#!/usr/bin/env python3
"""Compare the deployed and experimental prompts on hash-verified public photos only.

This prints raw model output. Never use private photos with this benchmark.
The input manifest format is documented in the adjacent README.
"""
import hashlib
import json
from pathlib import Path
import re
import sys
import time

import torch
from PIL import Image
from transformers import AutoModelForVision2Seq, AutoProcessor

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/concepts"))
import local_vlm_worker as worker

DEPLOYED_PROMPT_20261122 = "List visible objects as individual words separated by commas. Do not write sentences."
EXPERIMENTAL_PROMPT = (
    "List visible objects using simple generic nouns, not brand names or text. "
    "Answer with a comma-separated list of individual words."
)


def main():
    sample = Path(sys.argv[1])
    mode = sys.argv[2]
    if mode not in ("deployed", "experimental"):
        raise ValueError("mode must be deployed or experimental")
    rows = json.loads((sample / "manifest.json").read_text())["rows"]
    if len(sys.argv) > 3:
        rows = rows[:int(sys.argv[3])]
    torch.set_num_threads(4)
    processor = worker.configure_processor(AutoProcessor.from_pretrained(
        "/opt/model", local_files_only=True, trust_remote_code=False
    ))
    model = AutoModelForVision2Seq.from_pretrained(
        "/opt/model", local_files_only=True, trust_remote_code=False
    ).eval()
    prompt = processor.apply_chat_template([{"role": "user", "content": [
        {"type": "image"},
        {"type": "text", "text": DEPLOYED_PROMPT_20261122 if mode == "deployed" else EXPERIMENTAL_PROMPT},
    ]}], add_generation_prompt=True)
    for row in rows:
        if not isinstance(row["index"], int) or row["index"] < 1 or not re.fullmatch(r"[0-9a-f]{16}", row["image_id"]):
            raise ValueError("invalid public sample identifier")
        path = sample / f"{row['index']:02d}-{row['image_id']}.jpg"
        with path.open("rb") as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != row["sha256"]:
                raise ValueError(f"public input checksum mismatch: {row['image_id']}")
        with Image.open(path) as source:
            inputs = processor(text=prompt, images=[source.convert("RGB")], return_tensors="pt")
        start = time.monotonic()
        with torch.inference_mode():
            generated = model.generate(**inputs, max_new_tokens=96, do_sample=False)
        output = processor.decode(generated[0][inputs["input_ids"].shape[-1]:], skip_special_tokens=True)
        print(json.dumps({"id": row["image_id"], "sha256": row["sha256"],
                          "seconds": round(time.monotonic() - start, 2),
                          "raw": output, "labels": worker.candidates(output)}), flush=True)


if __name__ == "__main__":
    main()
