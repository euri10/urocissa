#!/usr/bin/env python3
"""Test frozen Concepts and count public-photo coverage on disjoint Open Images test data.

Only aggregate reports belong in Git. Public images, manifests and embeddings remain
in the explicit scratch cache. Missing annotations stay unknown, never negative.
"""
import argparse
from collections import Counter
import csv
import hashlib
import json
from pathlib import Path
import statistics
import time

import calibrate as tool

TEST_URL = 'https://storage.googleapis.com/openimages/v7/oidv7-test-annotations-human-imagelabels.csv'
TEST_SHA = '917d3ca8c0a2e09d652fb3148e9264df6117bfcaccf425a60a29b23269487953'


def ranked(ids, seed):
    return sorted(ids, key=lambda image: tool.digest(f'{seed}:{image}'.encode()))


def displayed(catalog, scores):
    """Mirror the existing no-feedback engine: active thresholds, ancestors, cap."""
    concepts = {c['id']: c for c in catalog['concepts']}
    fired = {c['id'] for c in concepts.values()
             if c['threshold'] is not None and scores[c['id']] >= c['threshold']}
    ancestors = set()
    pending = [parent for name in fired for parent in concepts[name]['parents']]
    while pending:
        parent = pending.pop()
        if parent not in ancestors:
            ancestors.add(parent)
            pending.extend(concepts[parent]['parents'])
    return sorted(fired - ancestors, key=lambda name: (-scores[name], name))[:8]


def run(args):
    deadline = time.monotonic() + args.deadline

    def remaining():
        seconds = deadline - time.monotonic()
        if seconds <= 0:
            raise TimeoutError('held-out evaluation deadline expired; public cache retained')
        return min(30, seconds)

    args.cache.mkdir(parents=True, exist_ok=True)
    labels_path = args.cache / 'test-labels.csv'
    if not labels_path.exists():
        # The official CSV is public. Stream it without keeping another copy in RAM.
        import urllib.request
        with urllib.request.urlopen(TEST_URL, timeout=60) as source:
            with labels_path.with_suffix('.partial').open('wb') as target:
                while block := source.read(1024 * 1024):
                    remaining()
                    target.write(block)
        labels_path.with_suffix('.partial').replace(labels_path)
    hasher = hashlib.sha256()
    with labels_path.open('rb') as source:
        while block := source.read(1024 * 1024):
            hasher.update(block)
    if hasher.hexdigest() != TEST_SHA:
        raise ValueError('independent test annotations checksum mismatch')
    catalog, baseline = tool.read_json(args.catalog), tool.read_json(args.baseline)
    with (args.calibration_cache / 'classes.csv').open(newline='') as source:
        classes = {name: mid for mid, name in csv.reader(source)}
    by_name = {c['id']: c for c in catalog['concepts']}
    wanted = {classes[c['open_images']] for c in catalog['concepts'] if c['open_images'] in classes}
    labels = tool.annotations(labels_path, wanted)
    selected = {}
    for name in args.concepts:
        concept = by_name[name]
        if concept['threshold'] is None:
            raise ValueError(f'{name} has no frozen threshold')
        annotated = labels[classes[concept['open_images']]]
        selected[name] = [image for label in (0, 1)
                          for image in ranked((i for i, v in annotated.items() if v == label),
                                              'urocissa-everyday-test-v1')[:80]]
    all_ids = set()
    with labels_path.open(newline='') as source:
        for row in csv.DictReader(source):
            if row['Source'] in tool.HUMAN_SOURCES:
                all_ids.add(row['ImageID'])
    random_ids = ranked(all_ids, 'urocissa-coverage-test-v1')[:100]
    ids = sorted(set(random_ids).union(*selected.values()))
    calibration_ids = set(tool.read_json(args.calibration_cache / 'images.json'))
    if calibration_ids.intersection(ids):
        raise ValueError('independent sample overlaps calibration images')
    info = json.loads(tool.fetch(args.tagger + '/v1/info', timeout=remaining()))
    if info['model_id'] != catalog['model_id'] or baseline['model_id'] != catalog['model_id']:
        raise ValueError('held-out model mismatch')
    prompts = [catalog['prompt_template'].format(label=c['label_en']) for c in catalog['concepts']]
    texts = []
    for start in range(0, len(prompts), 64):
        response = json.loads(tool.fetch(args.tagger + '/v1/embed/text',
                              json.dumps({'texts': prompts[start:start + 64]}).encode(), 'application/json', remaining()))
        if response['model_id'] != info['model_id'] or len(response['embeddings']) != len(prompts[start:start + 64]):
            raise ValueError('held-out text model/count mismatch')
        texts.extend(tool.validate_vector(v, 768) for v in response['embeddings'])
    image_dir = args.cache / 'images'
    image_dir.mkdir(exist_ok=True)
    scores, hashes = {}, {}
    for index, image in enumerate(ids, 1):
        path = image_dir / f'{image}.jpg'
        if not path.exists():
            path.write_bytes(tool.fetch(f'https://open-images-dataset.s3.amazonaws.com/test/{image}.jpg', timeout=remaining()))
        data = path.read_bytes()
        hashes[image] = tool.digest(data)
        response = json.loads(tool.fetch(args.tagger + '/v1/embed/image', data, 'image/jpeg', remaining()))
        if response['model_id'] != info['model_id']:
            raise ValueError('held-out image model mismatch')
        vector = tool.validate_vector(response['embedding'], 768)
        scores[image] = {c['id']: tool.probability(vector, text, info)
                         for c, text in zip(catalog['concepts'], texts, strict=True)}
        if index % 25 == 0 or index == len(ids):
            print(f'Independent test: {index}/{len(ids)} public images', flush=True)
    if json.loads(tool.fetch(args.tagger + '/v1/info', timeout=remaining())) != info:
        raise ValueError('held-out sidecar identity changed')
    categories = {name: tool.metrics([(scores[i][name], labels[classes[by_name[name]['open_images']]][i])
                                      for i in sample], by_name[name]['threshold'])
                  for name, sample in selected.items()}
    for result in categories.values():
        result['accepted'] = tool.enough(result, 5) and tool.passes(result, 0.9, 0.1, 5)
    coverage = {}
    for name, definition in [('baseline', baseline), ('candidate', catalog)]:
        counts, annotation = [], Counter({'positive': 0, 'negative': 0, 'unknown': 0})
        for image in random_ids:
            tags = displayed(definition, scores[image])
            counts.append(len(tags))
            for tag in tags:
                mid = classes.get(by_name[tag]['open_images'])
                value = labels.get(mid, {}).get(image)
                annotation[{1: 'positive', 0: 'negative', None: 'unknown'}[value]] += 1
        coverage[name] = {'photos': len(counts), 'distribution': {i: counts.count(i) for i in range(9)},
                          'mean': statistics.mean(counts), 'median': statistics.median(counts),
                          'annotations': dict(annotation)}
    tool.write_json(args.cache / 'images.json', hashes)
    tool.write_json(args.cache / 'scores.json', scores)
    report = {'schema_version': 1, 'dataset': {'url': TEST_URL, 'sha256': TEST_SHA},
              'model': info, 'catalog_sha256': tool.digest(args.catalog.read_bytes()),
              'baseline_sha256': tool.digest(args.baseline.read_bytes()),
              'sample_sha256': tool.digest(json.dumps({'categories': selected, 'random': random_ids}, sort_keys=True).encode()),
              'images_sha256': tool.digest(json.dumps(hashes, sort_keys=True).encode()),
              'calibration_overlap': 0, 'categories': categories, 'coverage': coverage,
              'limitations': 'Public human-label-conditional class samples; unknown labels excluded. '
                             'Not owner-library precision or proof of model pretraining independence.'}
    tool.write_json(args.output, report)
    print(json.dumps({'categories': categories, 'coverage': coverage}, sort_keys=True), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['catalog', 'baseline', 'calibration-cache', 'cache', 'output']:
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--tagger', required=True)
    parser.add_argument('--concepts', nargs='*', default=[])
    parser.add_argument('--deadline', type=int, default=7200)
    run(parser.parse_args())
