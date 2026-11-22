import socket
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

import local_vlm_worker
from local_vlm_worker import candidates, receive, socket_lease


class LocalVlmWorkerTests(unittest.TestCase):
    def test_processor_uses_a_single_image_patch(self):
        processor = SimpleNamespace(image_processor=SimpleNamespace(do_image_splitting=True))
        self.assertIs(local_vlm_worker.configure_processor(processor), processor)
        self.assertFalse(processor.image_processor.do_image_splitting)

    def test_candidates_are_bounded_deduplicated_and_filter_attributes(self):
        self.assertEqual(
            candidates("Tree, tree, rhubarb, elderly woman, 42, garden, black cat."),
            ["tree", "rhubarb", "garden", "cat"],
        )
        self.assertEqual(candidates("a\nb"), [])
        self.assertEqual(candidates("a" * 8193), [])
        self.assertEqual(len(candidates("tree, plant, water, flower, garden, cat, dog, house, sky")), 8)
        self.assertEqual(candidates("woman, black person, old man, person, snails, plant, rhubarb"), ["person", "snail", "plant", "rhubarb"])
        self.assertEqual(candidates("there is a cat, cat wearing a hat, famous person"), [])

    def test_receive_requires_the_complete_frame(self):
        left, right = socket.socketpair()
        try:
            left.sendall(b"abc")
            left.shutdown(socket.SHUT_WR)
            self.assertEqual(receive(right, 3), b"abc")
            with self.assertRaises(ValueError):
                receive(right, 1)
        finally:
            left.close()
            right.close()

    def test_socket_lease_preserves_active_and_untrusted_paths_and_recovers_stale_socket(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "worker.sock"
            listener = socket.socket(socket.AF_UNIX)
            listener.bind(str(path))
            listener.listen()
            with self.assertRaises(ValueError), socket_lease(path):
                pass
            listener.close()
            with socket_lease(path):
                self.assertFalse(path.exists())
                with self.assertRaises(BlockingIOError), socket_lease(path):
                    pass
            path.write_text("do not replace", encoding="utf-8")
            with self.assertRaises(ValueError), socket_lease(path):
                pass
            self.assertEqual(path.read_text(encoding="utf-8"), "do not replace")


if __name__ == "__main__":
    unittest.main()
