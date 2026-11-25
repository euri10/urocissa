import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import visual_precision as report


class VisualPrecisionTests(unittest.TestCase):
    def test_uncertain_is_penalized_and_counts_include_empty_photos(self):
        predictions = [{'id': 'a', 'labels': ['bird', 'tree']}, {'id': 'b', 'labels': []}]
        reviews = {'a': {'bird': 'correct', 'tree': 'uncertain'}, 'b': {}}
        result = report.summarize(predictions, reviews)
        self.assertEqual(result['judgments'], {'correct': 1, 'incorrect': 0, 'uncertain': 1})
        self.assertEqual(result['conservative_precision'], 0.5)
        self.assertEqual(result['tags_per_photo_mean'], 1)
        self.assertEqual(result['distribution']['0'], 1)
        self.assertFalse(result['public_screen_passed'])

    def test_fixed_precision_and_minimum_support_gates(self):
        predictions = [{'id': str(n), 'labels': ['tree']} for n in range(50)]
        reviews = {str(n): {'tree': 'correct' if n < 45 else 'incorrect'} for n in range(50)}
        self.assertTrue(report.summarize(predictions, reviews)['public_screen_passed'])
        reviews['44']['tree'] = 'uncertain'
        self.assertFalse(report.summarize(predictions, reviews)['public_screen_passed'])
        reviews['44']['tree'] = 'correct'
        predictions.pop()
        reviews.pop('49')
        self.assertFalse(report.summarize(predictions, reviews)['public_screen_passed'])

    def test_incomplete_duplicate_or_malformed_review_is_rejected(self):
        predictions = [{'id': 'a', 'labels': ['bird']}]
        for reviews in ({}, {'a': {}}, {'a': {'bird': 'unknown'}},
                        {'a': {'bird': 'correct'}, 'b': {}}):
            with self.subTest(reviews=reviews), self.assertRaises(ValueError):
                report.summarize(predictions, reviews)
        for rows in ([{'id': 'a', 'labels': ['bird', 'bird']}], predictions * 2,
                     [{'id': 'a', 'labels': ['x'] * 9}]):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                report.summarize(rows, {'a': {'bird': 'correct'}})

    def test_zero_predictions_never_pass(self):
        result = report.summarize([{'id': 'a', 'labels': []}], {'a': {}})
        self.assertEqual(result['conservative_precision'], 0)
        self.assertFalse(result['public_screen_passed'])

    def test_cli_reports_only_aggregates(self):
        with tempfile.TemporaryDirectory() as directory:
            predictions = Path(directory, 'predictions.json')
            reviews = Path(directory, 'reviews.json')
            predictions.write_text(json.dumps([{'id': 'public-image-id', 'labels': ['public-label']}]))
            reviews.write_text(json.dumps({'public-image-id': {'public-label': 'correct'}}))
            result = subprocess.run([sys.executable, '-B', report.__file__, '--predictions', str(predictions),
                                     '--reviews', str(reviews)], capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 1)  # Below minimum support.
            self.assertEqual(json.loads(result.stdout)['judgments']['correct'], 1)
            self.assertNotIn('public-image-id', result.stdout + result.stderr)
            self.assertNotIn('public-label', result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
