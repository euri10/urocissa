#!/usr/bin/env python3
"""Calibrate built-in Concepts with the production sidecar; Python standard library only.

Downloads and embeddings stay in --cache. Only the generated catalog and aggregate
report belong in Git. Missing human labels are unknown, never negative examples.
"""
import argparse
import concurrent.futures
import csv
import hashlib
import json
import math
from pathlib import Path
import struct
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
HUMAN_SOURCES = {"verification", "crowdsource-verification"}
SEED = "urocissa-concepts-v1"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".partial")
    temporary.write_text(json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n")
    temporary.replace(path)


def read_json(path):
    return json.loads(path.read_text())


def rank(image_id):
    return digest(f"{SEED}:{image_id}".encode())


def partition(image_id):
    # Independent of sampling order, class, labels and model scores. A photo can
    # never occur on both sides, even if it is annotated for several Concepts.
    return "selection" if int(digest(f"split:{image_id}".encode())[:2], 16) < 128 else "evaluation"


def sample_images(labels, limit):
    # Do not refill a scarce class from the majority: that recreated the
    # positive-enriched Cloud pool that accepted an always-on classifier.
    if limit < 2 or limit % 2:
        raise ValueError("sample size must be an even number of at least two")
    return sorted((image_id for label in (0, 1)
                   for image_id in sorted((i for i, value in labels.items() if value == label),
                                          key=rank)[:limit // 2]), key=rank)


def annotations(path, wanted):
    result = {label: {} for label in wanted}
    with path.open(newline="") as source:
        for row in csv.DictReader(source):
            label = row["LabelName"]
            if label not in result or row["Source"] not in HUMAN_SOURCES:
                continue
            confidence = float(row["Confidence"])
            if confidence not in (0, 1):
                raise ValueError("human confidence must be exactly zero or one")
            previous = result[label].get(row["ImageID"])
            if previous is not None and previous != confidence:
                raise ValueError("contradictory human annotations")
            result[label][row["ImageID"]] = int(confidence)
    return result


def metrics(rows, threshold):
    positives = sum(label for _, label in rows)
    negatives = len(rows) - positives
    predicted = [(score, label) for score, label in rows if score >= threshold]
    tp = sum(label for _, label in predicted)
    recall = tp / positives if positives else 0.0
    false_positive_rate = (len(predicted) - tp) / negatives if negatives else 0.0
    # Equal class priors prevent a positive-enriched annotation pool from making
    # an always-on classifier pass. Keep raw precision as a separate gate too.
    balanced_precision = (recall / (recall + false_positive_rate)
                          if positives and negatives and recall + false_positive_rate else 0.0)
    return {"positive": positives, "negative": negatives,
            "predicted": len(predicted), "true_positive": tp,
            "precision": tp / len(predicted) if predicted else 0.0,
            "balanced_precision": balanced_precision,
            "false_positive_rate": false_positive_rate, "recall": recall}


def enough(result, minimum):
    return result["positive"] >= minimum and result["negative"] >= minimum


def passes(result, precision, recall, minimum):
    return (result["predicted"] >= minimum and result["precision"] >= precision
            and result["balanced_precision"] >= precision
            and result["recall"] >= recall)


def select_threshold(rows, precision, recall, minimum):
    # Group tied scores before evaluating, so input order cannot inflate precision.
    # Midpoints leave room for f32 dot-product rounding in the Rust engine.
    scores = sorted({score for score, _ in rows}, reverse=True)
    candidates = []
    for index, score in enumerate(scores):
        lower = scores[index + 1] if index + 1 < len(scores) else 0.0
        threshold = f32((score + lower) / 2)
        if not 0 < threshold < 1:
            continue
        result = metrics(rows, threshold)
        if passes(result, precision, recall, minimum):
            candidates.append((result["recall"], result["balanced_precision"],
                               result["precision"], threshold))
    return max(candidates)[3] if candidates else None


def calibrate(rows, precision=0.9, recall=0.1, minimum=5):
    selection = [(r["score"], r["label"]) for r in rows if r["partition"] == "selection"]
    evaluation = [(r["score"], r["label"]) for r in rows if r["partition"] == "evaluation"]
    baseline = {part: metrics(values, 1.0) for part, values in
                [("selection", selection), ("evaluation", evaluation)]}
    if not all(enough(result, minimum) for result in baseline.values()):
        return {"status": "dropped", "reason": "insufficient_support", "threshold": None, **baseline}
    threshold = select_threshold(selection, precision, recall, minimum)
    if threshold is None:
        return {"status": "dropped", "reason": "selection_quality", "threshold": None, **baseline}
    results = {part: metrics(values, threshold) for part, values in
               [("selection", selection), ("evaluation", evaluation)]}
    accepted = passes(results["evaluation"], precision, recall, minimum)
    return {"status": "calibrated" if accepted else "dropped",
            "reason": None if accepted else "evaluation_quality",
            "threshold": threshold if accepted else None,
            "selected_threshold": threshold, **results}


def f32(value):
    return struct.unpack("f", struct.pack("f", value))[0]


def probability(image, text, info):
    dot = f32(sum(a * b for a, b in zip(image, text, strict=True)))
    logit = f32(f32(dot * f32(info["logit_scale"])) + f32(info["logit_bias"]))
    if logit >= 0:
        return f32(1 / (1 + math.exp(-logit)))
    exp = math.exp(logit)
    return f32(exp / (1 + exp))


def validate_vector(vector, dim):
    if (len(vector) != dim or any(not math.isfinite(v) for v in vector)
            or abs(math.sqrt(sum(v * v for v in vector)) - 1) > 0.001):
        raise ValueError("sidecar returned an invalid vector")
    return vector


def fetch(url, data=None, content_type=None, timeout=30):
    request = urllib.request.Request(url, data=data)
    if content_type:
        request.add_header("Content-Type", content_type)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def run(args):
    deadline = time.monotonic() + args.deadline

    def remaining():
        seconds = deadline - time.monotonic()
        if seconds <= 0:
            raise TimeoutError("calibration deadline expired; cache retained for resume")
        return min(30, seconds)

    catalog = read_json(args.catalog)
    # Thresholds are outputs. Excluding them makes the committed calibrated file
    # a reproducible input to its own next run, without changing the report hash.
    definitions = {**catalog, "concepts": [{k: v for k, v in c.items() if k != "threshold"}
                                          for c in catalog["concepts"]]}
    definition_hash = digest(json.dumps(definitions, sort_keys=True).encode())
    lock = read_json(args.dataset_lock)
    args.cache.mkdir(parents=True, exist_ok=True)
    for kind in ["labels", "classes"]:
        path = args.cache / f"{kind}.csv"
        if not path.exists():
            path.write_bytes(fetch(lock[kind]["url"], timeout=remaining()))
        if digest(path.read_bytes()) != lock[kind]["sha256"]:
            raise ValueError(f"{kind} dataset checksum mismatch")
    with (args.cache / "classes.csv").open(newline="") as source:
        classes = {name: mid for mid, name in csv.reader(source)}
    concepts = catalog["concepts"]
    labels = annotations(args.cache / "labels.csv", {
        classes[c["open_images"]] for c in concepts if c["open_images"] in classes})
    samples = {c["id"]: sample_images(labels.get(classes.get(c["open_images"]), {}),
                                     args.sample_per_concept) for c in concepts}
    image_ids = sorted({image_id for ids in samples.values() for image_id in ids})
    info = json.loads(fetch(args.tagger + "/v1/info", timeout=remaining()))
    if (info["model_id"] != catalog["model_id"] or info["embedding_dim"] != 768
            or not math.isfinite(info["logit_scale"]) or info["logit_scale"] <= 0
            or not math.isfinite(info["logit_bias"])):
        raise ValueError("sidecar does not match the catalog's pinned model")
    identity = {key: info[key] for key in ["model_id", "revision", "precision", "embedding_dim",
                "image_size", "text_max_tokens", "logit_scale", "logit_bias", "runtime", "tagger_version", "device"]}
    inference = args.cache / digest(json.dumps(identity, sort_keys=True).encode())[:16]
    inference.mkdir(exist_ok=True)
    write_json(inference / "identity.json", identity)
    prompts = [catalog["prompt_template"].format(label=c["label_en"]) for c in concepts]
    text_vectors = []
    for start in range(0, len(prompts), 64):
        response = json.loads(fetch(args.tagger + "/v1/embed/text",
                              json.dumps({"texts": prompts[start:start + 64]}).encode(),
                              "application/json", remaining()))
        if response["model_id"] != info["model_id"] or len(response["embeddings"]) != len(prompts[start:start + 64]):
            raise ValueError("sidecar changed model or text batch length")
        text_vectors.extend(validate_vector(v, 768) for v in response["embeddings"])
    image_dir = args.cache / "images"
    image_dir.mkdir(exist_ok=True)

    def download(image_id):
        path = image_dir / f"{image_id}.jpg"
        if not path.exists():
            data = fetch(lock["images"].format(image_id=image_id), timeout=remaining())
            # Write only complete downloads; an interrupted run can resume safely.
            temp = path.with_suffix(".partial")
            temp.write_bytes(data)
            temp.replace(path)
        return image_id, digest(path.read_bytes())

    print(f"Selected {len(image_ids)} images for {len(concepts)} Concepts", flush=True)
    hashes, vectors = {}, {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        for count, (image_id, sha256) in enumerate(pool.map(download, image_ids), 1):
            remaining()
            hashes[image_id] = sha256
            path = inference / f"{image_id}.json"
            if path.exists():
                response = read_json(path)
            else:
                response = json.loads(fetch(args.tagger + "/v1/embed/image",
                                      (image_dir / f"{image_id}.jpg").read_bytes(),
                                      "image/jpeg", remaining()))
                response["image_sha256"] = hashes[image_id]
                write_json(path, response)
            if response["model_id"] != info["model_id"] or response["image_sha256"] != hashes[image_id]:
                raise ValueError("embedding cache model/image mismatch")
            vectors[image_id] = validate_vector(response["embedding"], 768)
            if count % 50 == 0 or count == len(image_ids):
                print(f"Downloaded/embedded/verified {count}/{len(image_ids)}", flush=True)
    write_json(args.cache / "images.json", hashes)
    if json.loads(fetch(args.tagger + "/v1/info", timeout=remaining())) != info:
        raise ValueError("sidecar identity changed during calibration")
    report = {"schema_version": 2, "vocabulary_version": catalog["version"], "dataset": lock,
              "tool_sha256": digest(Path(__file__).read_bytes()),
              "catalog_definition_sha256": definition_hash, "model": info,
              "method": {"seed": SEED, "sample_per_concept": args.sample_per_concept,
                         "sampling": "hash-ordered explicit positives and negatives, capped at half the budget each; no refill",
                         "target_precision": args.precision, "minimum_recall": args.recall,
                         "precision_gates": ["observed", "equal_prior"],
                         "equal_prior_precision": "TPR / (TPR + FPR); 0 without both classes or predictions",
                         "minimum_per_class_per_partition": args.minimum,
                         "minimum_predictions_per_partition": args.minimum,
                         "unknown_labels": "ignored", "split": "sha256(split:ImageID) first byte < 128",
                         "limitation": "conditional on non-exhaustive human labels; not a production precision guarantee"},
              "images": {"count": len(image_ids), "sha256": digest(json.dumps(hashes, sort_keys=True).encode())},
              "concepts": {}}
    for concept, text in zip(concepts, text_vectors, strict=True):
        mid = classes.get(concept["open_images"])
        rows = [{"image_id": image_id, "partition": partition(image_id), "label": labels[mid][image_id],
                 "score": probability(vectors[image_id], text, info)} for image_id in samples[concept["id"]]]
        result = calibrate(rows, args.precision, args.recall, args.minimum)
        if mid is None:
            result["reason"] = "no_open_images_class"
        result["open_images_mid"] = mid
        result["sample_sha256"] = digest(json.dumps(rows, sort_keys=True).encode())
        report["concepts"][concept["id"]] = result
        concept["threshold"] = result["threshold"]
    args.output.mkdir(parents=True, exist_ok=True)
    write_json(args.output / "concepts.json", catalog)
    write_json(args.output / "calibration-report.json", report)
    accepted = sum(c["threshold"] is not None for c in concepts)
    print(f"Calibrated {accepted}/{len(concepts)}; report: {args.output / 'calibration-report.json'}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, default=ROOT / "gallery-backend/assets/concepts.json")
    parser.add_argument("--dataset-lock", type=Path, default=Path(__file__).with_name("dataset.lock.json"))
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--tagger", required=True, help="explicit existing stateless sidecar URL")
    parser.add_argument("--sample-per-concept", type=int, default=160)
    parser.add_argument("--precision", type=float, default=0.9)
    parser.add_argument("--recall", type=float, default=0.1)
    parser.add_argument("--minimum", type=int, default=5)
    parser.add_argument("--deadline", type=int, default=7200)
    args = parser.parse_args()
    if (args.sample_per_concept < 2 or args.sample_per_concept % 2
            or args.minimum <= 0 or args.deadline <= 0
            or not 0 < args.precision <= 1 or not 0 < args.recall <= 1):
        parser.error("sample size must be even and >=2; support/deadline positive; precision/recall in (0, 1]")
    run(args)


if __name__ == "__main__":
    main()
