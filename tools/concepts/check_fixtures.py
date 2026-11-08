#!/usr/bin/env python3
"""Probe public/synthetic regression images with the exact-model stateless sidecar.

These reviewed absences are a release check, never threshold-selection data.
Unlisted image/Concept pairs remain unknown. See the existing fixture attribution.
"""
import argparse
import json
import math
from pathlib import Path

import calibrate as tool

IMAGES = tool.ROOT / "gallery-tagger/tests/fixtures/images"
ABSENT = {
    "cat.jpg": ["cloud", "snow"],
    "stork_pair.jpg": ["snow"],
    "stork_fledglings.jpg": ["snow"],
    "bicycles.jpg": ["snow"],
    "sunset.jpg": ["snow"],
    "screenshot.jpg": ["cat", "bird"],
    "synthetic_gray.jpg": ["cat", "bird", "cloud", "snow"],
    "synthetic_rgba.png": ["cat", "bird", "cloud", "snow"],
    "synthetic_tiny.png": ["cat", "bird", "cloud", "snow"],
}
PRESENT = {"cat.jpg": ["cat"], "stork_pair.jpg": ["bird"], "stork_fledglings.jpg": ["bird"]}


def prompt_hash(catalog):
    return tool.digest(json.dumps(
        [(c["id"], catalog["prompt_template"].format(label=c["label_en"]))
         for c in catalog["concepts"]], sort_keys=True).encode())


def probe(catalog, url):
    info = json.loads(tool.fetch(url + "/v1/info"))
    if (info["model_id"] != catalog["model_id"] or info["embedding_dim"] != 768
            or not math.isfinite(info["logit_scale"]) or info["logit_scale"] <= 0
            or not math.isfinite(info["logit_bias"])):
        raise ValueError("fixture probe model does not match the catalog")
    prompts = [catalog["prompt_template"].format(label=c["label_en"])
               for c in catalog["concepts"]]
    texts = []
    for start in range(0, len(prompts), 64):
        batch = prompts[start:start + 64]
        response = json.loads(tool.fetch(url + "/v1/embed/text",
                              json.dumps({"texts": batch}).encode(), "application/json"))
        if response["model_id"] != info["model_id"] or len(response["embeddings"]) != len(batch):
            raise ValueError("fixture probe text model or count changed")
        texts.extend(tool.validate_vector(v, info["embedding_dim"]) for v in response["embeddings"])
    observations = {"schema_version": 1, "model": info, "prompt_sha256": prompt_hash(catalog),
                    "purpose": "independent regression only; not threshold selection", "images": {}}
    for name in ABSENT:
        data = (IMAGES / name).read_bytes()
        content_type = "image/png" if name.endswith(".png") else "image/jpeg"
        response = json.loads(tool.fetch(url + "/v1/embed/image", data, content_type))
        if response["model_id"] != info["model_id"]:
            raise ValueError("fixture probe image model changed")
        image = tool.validate_vector(response["embedding"], info["embedding_dim"])
        observations["images"][name] = {
            "sha256": tool.digest(data),
            "scores": {c["id"]: tool.probability(image, text, info)
                       for c, text in zip(catalog["concepts"], texts, strict=True)}}
    if json.loads(tool.fetch(url + "/v1/info")) != info:
        raise ValueError("fixture probe sidecar identity changed")
    return observations


def failures(catalog, observations):
    if (observations["model"]["model_id"] != catalog["model_id"]
            or observations["prompt_sha256"] != prompt_hash(catalog)
            or set(observations["images"]) != set(ABSENT)):
        raise ValueError("fixture observation provenance mismatch")
    thresholds = {c["id"]: c["threshold"] for c in catalog["concepts"]}
    result = []
    for name, absent in ABSENT.items():
        observed = observations["images"][name]
        if observed["sha256"] != tool.digest((IMAGES / name).read_bytes()):
            raise ValueError(f"fixture image changed: {name}")
        if set(observed["scores"]) != set(thresholds):
            raise ValueError("fixture Concept coverage mismatch")
        if any(not math.isfinite(score) or not 0 <= score <= 1
               for score in observed["scores"].values()):
            raise ValueError("invalid fixture score")
        for concept in absent:
            score, threshold = observed["scores"][concept], thresholds[concept]
            if threshold is not None and score >= threshold:
                result.append(f"{name}: absent {concept} fires ({score} >= {threshold})")
        for concept in PRESENT.get(name, []):
            score, threshold = observed["scores"][concept], thresholds[concept]
            if threshold is None or score < threshold:
                result.append(f"{name}: expected {concept} does not fire")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, default=tool.ROOT / "gallery-backend/assets/concepts.json")
    parser.add_argument("--tagger", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    catalog = tool.read_json(args.catalog)
    observed = probe(catalog, args.tagger.rstrip("/"))
    tool.write_json(args.output, observed)
    rejected = failures(catalog, observed)
    for failure in rejected:
        print(failure)
    print(f"Checked {len(ABSENT)} independent fixture images; {len(rejected)} failures")
    raise SystemExit(bool(rejected))


if __name__ == "__main__":
    main()
