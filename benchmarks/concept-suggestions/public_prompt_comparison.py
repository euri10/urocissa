#!/usr/bin/env python3
"""Compare prompts on the fixed, attributed public fixtures; never use private photos."""
import json
import time
from pathlib import Path
import sys

import torch
from PIL import Image
from transformers import AutoModelForVision2Seq, AutoProcessor

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/concepts"))
import local_vlm_worker as worker

FIXTURES = ("bicycles.jpg", "cat.jpg", "stork_pair.jpg", "stork_fledglings.jpg", "sunset.jpg")
BASELINE = "What objects and surroundings are visible in this photo? Answer with a short comma-separated list."


def main():
    torch.set_num_threads(4)
    processor = worker.configure_processor(AutoProcessor.from_pretrained(
        "/opt/model", local_files_only=True, trust_remote_code=False
    ))
    model = AutoModelForVision2Seq.from_pretrained(
        "/opt/model", local_files_only=True, trust_remote_code=False
    ).eval()
    for fixture in FIXTURES:
        with Image.open(ROOT / "gallery-tagger/tests/fixtures/images" / fixture) as source:
            image = source.convert("RGB")
        for name, text in (("baseline", BASELINE), ("candidate", worker.PROMPT)):
            prompt = processor.apply_chat_template([{"role": "user", "content": [
                {"type": "image"}, {"type": "text", "text": text},
            ]}], add_generation_prompt=True)
            start = time.monotonic()
            inputs = processor(text=prompt, images=[image], return_tensors="pt")
            with torch.inference_mode():
                generated = model.generate(**inputs, max_new_tokens=96, do_sample=False)
            output = processor.decode(generated[0][inputs["input_ids"].shape[-1]:], skip_special_tokens=True)
            print(json.dumps({"fixture": fixture, "prompt": name,
                              "seconds": round(time.monotonic() - start, 3),
                              "raw": output, "labels": worker.candidates(output)}), flush=True)


if __name__ == "__main__":
    main()
