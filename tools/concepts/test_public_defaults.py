"""Release contract: retained old calibration, frozen new gates, no unknown negatives."""
import unittest
from pathlib import Path

import calibrate as tool


class PublicDefaultsTests(unittest.TestCase):
    def test_published_supplement_binds_exact_artifacts_and_unchanged_gates(self):
        root = tool.ROOT
        baseline = tool.read_json(root/'gallery-backend/assets/concepts.json')
        path = root/'gallery-backend/assets/concept-defaults.json'
        defaults = tool.read_json(path)
        report = tool.read_json(Path(__file__).with_name('public-defaults-report.json'))
        self.assertEqual(report['baseline_catalog_sha256'], tool.digest((root/'gallery-backend/assets/concepts.json').read_bytes()))
        self.assertEqual(report['defaults_sha256'], tool.digest(path.read_bytes()))
        self.assertEqual(defaults['baseline_version'],baseline['version'])
        self.assertEqual(report['vocabulary_version'], defaults['vocabulary_version'])
        self.assertEqual(report['model']['model_id'],baseline['model_id'])
        self.assertEqual(defaults['model_id'],baseline['model_id'])
        self.assertEqual(defaults['embedding_dim'],768)
        self.assertEqual(report['gates'],{'observed_precision':.9,'equal_prior_precision':.9,
                         'recall':.1,'minimum_per_class':5,'minimum_predictions':5})
        names = set(report['zero_shot']) | set(report['references'])
        self.assertEqual(names, {'bench','bookcase','fence','insect','road','rose','snow','spider'})
        self.assertEqual(set(defaults['heads']),set(report['references']))
        original = {c['id']:c for c in baseline['concepts']}
        added = {c['id']:c for c in defaults['concepts']}
        self.assertFalse(set(original) & set(added))
        self.assertEqual(set(added),names-{'snow'})
        self.assertEqual(defaults['parent_refinements'],{'bee':['insect'],'butterfly':['insect']})
        self.assertEqual(report['fixtures']['vetoes'],[])
        self.assertEqual(set(report['fixtures']['qualified']),names)
        for name in names:
            result = (report['references'] | report['zero_shot'])[name]
            for part in ['selection','evaluation']:
                self.assertTrue(tool.enough(result[part],5))
                self.assertTrue(tool.passes(result[part],.9,.1,5))
            measured = report['independent_test']['categories'][name]
            self.assertTrue(measured['accepted'])
            self.assertTrue(tool.enough(measured,5))
            self.assertTrue(tool.passes(measured,.9,.1,5))
            self.assertEqual(measured,report['qualified_test']['categories'][name])
            if name in defaults['heads']:
                self.assertIsNone((original | added)[name]['threshold'])
                self.assertEqual(defaults['heads'][name]['threshold'],result['threshold'])
            else:
                self.assertEqual(added[name]['threshold'],result['threshold'])
        self.assertEqual(report['qualified_test']['calibration_overlap'],0)
        self.assertEqual(set(report['qualified_test']['categories']),names)
        failed = {k for k,v in report['independent_test']['categories'].items() if not v['accepted']}
        self.assertEqual(len(failed),11)
        self.assertFalse(names & failed)


if __name__ == '__main__':
    unittest.main()
