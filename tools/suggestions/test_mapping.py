import unittest

from mapping import candidates


class FrozenMappingTests(unittest.TestCase):
    def test_hierarchy_quarantine_neutral_people_and_deduplication(self):
        self.assertEqual(
            candidates(
                ["animal", "bird", "duck", "man", "women", "flower", "ground"],
                ["duck", "a rose", "sunflowers", "a dog"],
                "A woman and a cat.",
            ),
            ["duck", "person", "dog", "cat"],
        )

    def test_phrase_boundaries_exclusions_order_and_cap(self):
        self.assertEqual(
            candidates(
                ["no dog", "toy cat", "dog-shaped", "apple", "boat", "book", "chair"],
                ["table", "tree", "person", "bicycle", "house"],
                "A horse. Maybe a sheep.",
            ),
            ["apple", "boat", "book", "chair", "table", "tree", "person", "bicycle"],
        )


if __name__ == "__main__":
    unittest.main()
