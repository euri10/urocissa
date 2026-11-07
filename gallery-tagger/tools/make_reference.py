#!/usr/bin/env python3
"""Compute FP32 reference embeddings for the parity fixtures with transformers.

Usage: make_reference.py --out FILE [--variant NAME] [--source-dir DIR]
"""
import argparse
import json
import pathlib
import sys
import tempfile

import torch
import transformers
from PIL import Image

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from common import TEXT_MAX_TOKENS, TOOLS, fetch_verified, variant_spec

FIXTURES = TOOLS.parent / "tests" / "fixtures"
TEXTS = [
    "a photo of a stork",
    "une photo d'une cigogne",
    "a photo of a cat",
    "une photo d'un chat",
    "people riding bicycles",
    "des gens à vélo",
    "sunset over the ocean",
    "coucher de soleil sur la mer",
    "a screenshot of a photo gallery website",
    "Kid on a Bike at DUSK",
    "🐦 oiseau",
    " ".join(["a very long caption about storks nesting on a chimney in alsace"] * 12),
]


def rounded(vector: torch.Tensor) -> list[float]:
    vector = vector / vector.norm(p=2)
    return [round(float(value), 6) for value in vector]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=pathlib.Path, required=True)
    parser.add_argument("--variant")
    parser.add_argument("--source-dir", type=pathlib.Path)
    args = parser.parse_args()

    variant, spec = variant_spec(args.variant)
    with tempfile.TemporaryDirectory() as tmp:
        staging = fetch_verified(spec, args.source_dir, pathlib.Path(tmp))
        model = transformers.AutoModel.from_pretrained(staging, dtype=torch.float32).eval()
        image_processor = transformers.SiglipImageProcessorPil.from_pretrained(staging)
        tokenizer = transformers.AutoTokenizer.from_pretrained(staging)

    images = {}
    with torch.no_grad():
        for path in sorted((FIXTURES / "images").iterdir()):
            with Image.open(path) as image:
                pixels = image_processor(images=image.convert("RGB"), return_tensors="pt")["pixel_values"]
            images[path.name] = rounded(model.vision_model(pixel_values=pixels).pooler_output[0])
        texts = []
        for text in TEXTS:
            ids = tokenizer(
                [text.lower()], padding="max_length", max_length=TEXT_MAX_TOKENS, truncation=True, return_tensors="pt"
            )["input_ids"]
            embedding = model.text_model(input_ids=ids).pooler_output[0]
            texts.append({"text": text, "input_ids": ids[0].tolist(), "embedding": rounded(embedding)})

    reference = {
        "variant": variant,
        "revision": spec["revision"],
        "logit_scale": float(model.logit_scale.exp().item()),
        "logit_bias": float(model.logit_bias.item()),
        "generated_with": {"torch": torch.__version__, "transformers": transformers.__version__},
        "images": images,
        "texts": texts,
    }
    args.out.write_text(json.dumps(reference, separators=(",", ":")) + "\n")
    print(f"wrote {args.out} ({len(images)} images, {len(texts)} texts)")


if __name__ == "__main__":
    main()
