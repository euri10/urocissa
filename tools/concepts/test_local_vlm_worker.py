import hashlib
import os
import socket
import tempfile
import unittest
from contextlib import nullcontext
from pathlib import Path
from types import SimpleNamespace

import local_vlm_worker
from local_vlm_worker import candidates, receive, socket_lease


class LocalVlmWorkerTests(unittest.TestCase):
    def test_inference_guard_stops_at_deadline_or_client_disconnect(self):
        left, right = socket.socketpair()
        try:
            guard = local_vlm_worker.InferenceGuard(left, seconds=30)
            self.assertFalse(guard(None, None))
            self.assertTrue(local_vlm_worker.InferenceGuard(left, seconds=0)(None, None))
            right.close()
            self.assertTrue(guard(None, None))
        finally:
            left.close()
            right.close()

    def test_cancelled_inference_returns_no_partial_output(self):
        left, right = socket.socketpair()
        try:
            guard = local_vlm_worker.InferenceGuard(left, seconds=30)
            class FakeModel:
                def generate(self, **kwargs):
                    right.close()
                    self.stopped = kwargs["stopping_criteria"][0](None, None)
                    return "partial"
            model = FakeModel()
            with self.assertRaises(TimeoutError):
                local_vlm_worker.generate_checked(model, {}, guard, nullcontext)
            self.assertTrue(model.stopped)
        finally:
            left.close()
            right.close()

    def test_abandoned_queued_request_never_starts_generation(self):
        left, right = socket.socketpair()
        try:
            right.close()
            guard = local_vlm_worker.InferenceGuard(left, seconds=30)
            class FakeModel:
                def generate(self, **_kwargs):
                    raise AssertionError("abandoned request must not generate")
            with self.assertRaises(TimeoutError):
                local_vlm_worker.generate_checked(FakeModel(), {}, guard, nullcontext)
        finally:
            left.close()

    def test_expired_generation_does_not_return_partial_output(self):
        left, right = socket.socketpair()
        try:
            guard = local_vlm_worker.InferenceGuard(left, seconds=30)
            class FakeModel:
                def generate(self, **_kwargs):
                    guard.deadline = 0
                    return "partial"
            with self.assertRaises(TimeoutError):
                local_vlm_worker.generate_checked(FakeModel(), {}, guard, nullcontext)
        finally:
            left.close()
            right.close()

    def test_processor_uses_a_single_image_patch(self):
        processor = SimpleNamespace(image_processor=SimpleNamespace(do_image_splitting=True))
        self.assertIs(local_vlm_worker.configure_processor(processor), processor)
        self.assertFalse(processor.image_processor.do_image_splitting)

    def test_prompt_requests_generic_objects_not_text_or_brands(self):
        self.assertIn("generic nouns", local_vlm_worker.PROMPT)
        self.assertIn("not brand names or text", local_vlm_worker.PROMPT)

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
        self.assertEqual(candidates("no bicycle, without a person, elderly woman"), [])

    @unittest.skipUnless(os.getenv("UROCISSA_CONCEPT_MODEL_DIR"), "requires local concept model")
    def test_real_model_returns_useful_labels_for_public_bicycle_photo(self):
        import torch
        from PIL import Image
        from transformers import AutoModelForVision2Seq, AutoProcessor

        model_dir = os.environ["UROCISSA_CONCEPT_MODEL_DIR"]
        torch.set_num_threads(4)
        processor = local_vlm_worker.configure_processor(
            AutoProcessor.from_pretrained(model_dir, local_files_only=True, trust_remote_code=False)
        )
        model = AutoModelForVision2Seq.from_pretrained(
            model_dir, local_files_only=True, trust_remote_code=False
        ).eval()
        prompt = processor.apply_chat_template([{"role": "user", "content": [
            {"type": "image"}, {"type": "text", "text": local_vlm_worker.PROMPT},
        ]}], add_generation_prompt=True)
        fixture = Path(__file__).resolve().parents[2] / "gallery-tagger/tests/fixtures/images/bicycles.jpg"
        with Image.open(fixture) as image:
            inputs = processor(text=prompt, images=[image.convert("RGB")], return_tensors="pt")
        with torch.inference_mode():
            generated = model.generate(**inputs, max_new_tokens=96, do_sample=False)
        output = processor.decode(generated[0][inputs["input_ids"].shape[-1]:], skip_special_tokens=True)
        self.assertTrue({"bicycle", "person"}.issubset(candidates(output)))

    @unittest.skipUnless(
        os.getenv("UROCISSA_CONCEPT_MODEL_DIR") and os.getenv("UROCISSA_CONCEPT_PUBLIC_CAR"),
        "requires local concept model and hash-verified public car photo",
    )
    def test_real_model_returns_generic_car_on_public_photo(self):
        import torch
        from PIL import Image
        from transformers import AutoModelForVision2Seq, AutoProcessor

        fixture = Path(os.environ["UROCISSA_CONCEPT_PUBLIC_CAR"])
        with fixture.open("rb") as stream:
            self.assertEqual(hashlib.file_digest(stream, "sha256").hexdigest(),
                             "b4c1b1065446adacaf4ac4e73ade0993bcfd8d4c1f6e76fe0aff15db6ceead46")
        model_dir = os.environ["UROCISSA_CONCEPT_MODEL_DIR"]
        torch.set_num_threads(4)
        processor = local_vlm_worker.configure_processor(
            AutoProcessor.from_pretrained(model_dir, local_files_only=True, trust_remote_code=False)
        )
        model = AutoModelForVision2Seq.from_pretrained(
            model_dir, local_files_only=True, trust_remote_code=False
        ).eval()
        prompt = processor.apply_chat_template([{"role": "user", "content": [
            {"type": "image"}, {"type": "text", "text": local_vlm_worker.PROMPT},
        ]}], add_generation_prompt=True)
        with Image.open(fixture) as image:
            inputs = processor(text=prompt, images=[image.convert("RGB")], return_tensors="pt")
        with torch.inference_mode():
            generated = model.generate(**inputs, max_new_tokens=96, do_sample=False)
        output = processor.decode(generated[0][inputs["input_ids"].shape[-1]:], skip_special_tokens=True)
        self.assertIn("car", candidates(output))

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
