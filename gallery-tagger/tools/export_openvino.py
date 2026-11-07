#!/usr/bin/env python3
"""Export the pinned SigLIP 2 towers to OpenVINO IR for gallery-tagger.

Writes vision.xml/.bin, text.xml/.bin, tokenizer.json and manifest.json to
--out. Every source file is verified against model.lock.json before use.

Usage: export_openvino.py --out DIR [--variant NAME] [--source-dir DIR]
"""
import argparse
import json
import pathlib
import shutil
import sys
import tempfile

import nncf
import openvino as ov
import torch
import transformers

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from common import TEXT_MAX_TOKENS, fetch_verified, model_id, variant_spec

# Vision weights FP16; text weights INT8 (asymmetric, per-channel). The text
# tower is dominated by the 256k-token embedding table; INT8 keeps parity at
# cosine >= 0.999 while cutting its memory and latency by more than half.
PRECISION = "fp16-text-int8"


class VisionTower(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.vision = model.vision_model

    def forward(self, pixel_values):
        return self.vision(pixel_values=pixel_values).pooler_output


class TextTower(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.text = model.text_model

    def forward(self, input_ids):
        return self.text(input_ids=input_ids).pooler_output


def name_ports(model: ov.Model, input_name: str) -> ov.Model:
    model.inputs[0].get_tensor().set_names({input_name})
    model.outputs[0].get_tensor().set_names({"embedding"})
    return model


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=pathlib.Path, required=True)
    parser.add_argument("--variant")
    parser.add_argument("--source-dir", type=pathlib.Path)
    args = parser.parse_args()

    variant, spec = variant_spec(args.variant)
    size = spec["image_size"]
    args.out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        staging = fetch_verified(spec, args.source_dir, pathlib.Path(tmp))
        model = transformers.AutoModel.from_pretrained(staging, dtype=torch.float32).eval()
        with torch.no_grad():
            vision = ov.convert_model(
                VisionTower(model),
                example_input=torch.zeros(1, 3, size, size),
                input=[([-1, 3, size, size], ov.Type.f32)],
            )
            text = ov.convert_model(
                TextTower(model),
                example_input=torch.zeros(1, TEXT_MAX_TOKENS, dtype=torch.int64),
                input=[([-1, TEXT_MAX_TOKENS], ov.Type.i64)],
            )
        ov.save_model(name_ports(vision, "pixel_values"), args.out / "vision.xml", compress_to_fp16=True)
        text = nncf.compress_weights(text, mode=nncf.CompressWeightsMode.INT8_ASYM)
        ov.save_model(name_ports(text, "input_ids"), args.out / "text.xml", compress_to_fp16=True)
        shutil.copyfile(staging / "tokenizer.json", args.out / "tokenizer.json")
        preprocessor = json.loads((staging / "preprocessor_config.json").read_text())
        manifest = {
            "schema_version": 1,
            "model_id": model_id(variant, spec, PRECISION),
            "variant": variant,
            "repo": spec["repo"],
            "revision": spec["revision"],
            "precision": PRECISION,
            "embedding_dim": int(model.config.vision_config.hidden_size),
            "image_size": size,
            "image_mean": preprocessor["image_mean"],
            "image_std": preprocessor["image_std"],
            "rescale_factor": preprocessor["rescale_factor"],
            "text_max_tokens": TEXT_MAX_TOKENS,
            "text_lowercase": True,
            "logit_scale": float(model.logit_scale.exp().item()),
            "logit_bias": float(model.logit_bias.item()),
            "source_sha256": spec["files"],
            "exported_with": {
                "torch": torch.__version__,
                "transformers": transformers.__version__,
                "openvino": ov.__version__,
                "nncf": nncf.__version__,
            },
        }
        (args.out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({k: manifest[k] for k in ("model_id", "embedding_dim", "logit_scale", "logit_bias")}))


if __name__ == "__main__":
    main()
