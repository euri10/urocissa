"""Coverage accounting must follow chip suppression without inventing labels."""
import unittest

import heldout


class HeldoutTest(unittest.TestCase):
    def test_ancestors_suppressed_and_unavailable_concepts_cannot_fill_chips(self):
        catalog = {'concepts': [
            {'id': 'building', 'threshold': None, 'parents': []},
            {'id': 'house', 'threshold': 0.8, 'parents': ['building']},
            {'id': 'bridge', 'threshold': 0.9, 'parents': []},
        ]}
        scores = {'building': 1.0, 'house': 0.9, 'bridge': 0.85}
        self.assertEqual(heldout.displayed(catalog, scores), ['house'])
        catalog['concepts'][0]['threshold'] = 0.5
        self.assertEqual(heldout.displayed(catalog, scores), ['house'])

    def test_cap_and_stable_ties_match_engine_order(self):
        catalog = {'concepts': [{'id': f'object{i}', 'threshold': 0.5, 'parents': []}
                                for i in range(10)]}
        scores = {c['id']: 0.9 for c in reversed(catalog['concepts'])}
        self.assertEqual(heldout.displayed(catalog, scores), [f'object{i}' for i in range(8)])


if __name__ == '__main__':
    unittest.main()
