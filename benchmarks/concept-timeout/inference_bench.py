#!/usr/bin/env python3
"""Compare public-fixture inference; never use private photos or production data."""
import argparse
import importlib.util
import json
from pathlib import Path
import resource
import time

import torch
from PIL import Image
from transformers import AutoModelForVision2Seq, AutoProcessor

ROOT = Path("/src")
spec = importlib.util.spec_from_file_location("worker", ROOT / "tools/concepts/local_vlm_worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--single-patch", action="store_true")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--fixture", choices=("cat.jpg", "stork_pair.jpg"), default="cat.jpg")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    torch.set_num_threads(4)
    processor = AutoProcessor.from_pretrained("/opt/model", local_files_only=True, trust_remote_code=False)
    if args.single_patch:
        processor = worker.configure_processor(processor)
    model = AutoModelForVision2Seq.from_pretrained("/opt/model", local_files_only=True, trust_remote_code=False).eval()
    messages = [{"role": "user", "content": [{"type": "image"}, {"type": "text", "text": worker.PROMPT}]}]
    prompt = processor.apply_chat_template(messages, add_generation_prompt=True)
    path = ROOT / "gallery-tagger/tests/fixtures/images" / args.fixture
    with Image.open(path) as source:
        image = source.convert("RGB").resize((1920, 1473))
    records = []
    for index in range(args.warmups + args.samples):
        start = time.perf_counter()
        inputs = processor(text=prompt, images=[image], return_tensors="pt")
        processed = time.perf_counter()
        with torch.inference_mode():
            output = model.generate(**inputs, max_new_tokens=96, do_sample=False)
        completed = time.perf_counter()
        tokens = output[0][inputs["input_ids"].shape[-1]:]
        labels = worker.candidates(processor.decode(tokens, skip_special_tokens=True))
        record = {"sample": index, "warmup": index < args.warmups,
                  "prepare_seconds": round(processed - start, 3),
                  "generate_seconds": round(completed - processed, 3),
                  "total_seconds": round(completed - start, 3),
                  "image_patches": inputs["pixel_values"].shape[1],
                  "input_tokens": inputs["input_ids"].shape[-1],
                  "output_tokens": len(tokens), "labels": labels,
                  "peak_rss_kib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}
        records.append(record)
        print(json.dumps(record), flush=True)
    Path(args.output).write_text(json.dumps({"fixture": args.fixture, "single_patch": args.single_patch, "records": records}, indent=2) + "\n")


if __name__ == "__main__":
    main()
