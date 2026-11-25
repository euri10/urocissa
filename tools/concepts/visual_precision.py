#!/usr/bin/env python3
"""Aggregate explicitly judged public-photo predictions without outputting image IDs.

This screen does not replace per-Concept, owner-acceptance or runtime gates.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import statistics


def summarize(predictions, reviews):
    if not isinstance(predictions, list) or not predictions or not isinstance(reviews, dict):
        raise ValueError('expected public predictions and reviews')
    seen = set()
    counts = []
    judgments = Counter(correct=0, incorrect=0, uncertain=0)
    for row in predictions:
        if not isinstance(row, dict) or not isinstance(row.get('id'), str) or not row['id']:
            raise ValueError('invalid public prediction row')
        image = row['id']
        labels = row.get('labels')
        if (image in seen or not isinstance(labels, list) or len(labels) > 8
                or any(not isinstance(label, str) or not label.strip() for label in labels)
                or len(labels) != len(set(labels))):
            raise ValueError('duplicate or invalid public prediction')
        seen.add(image)
        review = reviews.get(image)
        if (not isinstance(review, dict) or set(review) != set(labels)
                or any(value not in judgments for value in review.values())):
            raise ValueError('each predicted label needs an explicit judgment')
        counts.append(len(labels))
        judgments.update(review.values())
    if set(reviews) != seen:
        raise ValueError('reviews must match the public sample exactly')
    total = sum(counts)
    return {
        'photos': len(counts),
        'tags': total,
        'tags_per_photo_mean': total / len(counts),
        'tags_per_photo_median': statistics.median(counts),
        'distribution': {str(n): counts.count(n) for n in range(9)},
        'judgments': dict(judgments),
        'conservative_precision': judgments['correct'] / total if total else 0.0,
        'public_screen_passed': total >= 50 and 10 * judgments['correct'] >= 9 * total,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--predictions', type=Path, required=True, help='public image IDs and labels')
    parser.add_argument('--reviews', type=Path, required=True, help='explicit public-only judgments')
    args = parser.parse_args()
    try:
        predictions = json.loads(args.predictions.read_text())
        reviews = json.loads(args.reviews.read_text())
        result = summarize(predictions, reviews)
    except (OSError, ValueError, TypeError):
        parser.exit(2, 'Invalid or incomplete public evaluation input.\n')
    print(json.dumps(result, sort_keys=True))
    return 0 if result['public_screen_passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
