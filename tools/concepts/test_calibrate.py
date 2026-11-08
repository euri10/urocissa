"""Small synthetic calibration fixture, never evidence of real-model quality."""
import argparse
import csv
import http.server
import json
from pathlib import Path
import tempfile
import threading
import unittest

import calibrate as tool


def rows(selection_good=True, evaluation_good=True):
    return [{"score": 0.9 if (label == int(good)) else 0.1,
             "label": label, "partition": partition}
            for partition, good in [("selection", selection_good), ("evaluation", evaluation_good)]
            for label in (0, 1) for _ in range(10)]


class CalibrationTests(unittest.TestCase):
    def test_precision_recall_and_held_out_failure(self):
        result = tool.calibrate(rows())
        self.assertEqual(result["status"], "calibrated")
        self.assertEqual(result["evaluation"]["precision"], 1)
        self.assertEqual(result["evaluation"]["recall"], 1)
        failure = tool.calibrate(rows(evaluation_good=False))
        self.assertEqual(failure["reason"], "evaluation_quality")
        self.assertIsNone(failure["threshold"])
        self.assertEqual(failure["selected_threshold"], result["selected_threshold"])

    def test_ties_never_split_and_order_does_not_change_threshold(self):
        tied = [{**row, "score": 0.7} for row in rows()]
        self.assertEqual(tool.calibrate(tied)["reason"], "selection_quality")
        self.assertEqual(tool.calibrate(rows()), tool.calibrate(list(reversed(rows()))))

    def test_both_partitions_need_positive_negative_and_prediction_support(self):
        self.assertEqual(tool.calibrate(rows()[:3])["reason"], "insufficient_support")
        positives_only = [row for row in rows() if row["label"]]
        self.assertEqual(tool.calibrate(positives_only)["reason"], "insufficient_support")
        almost_no_recall = rows()
        for row in almost_no_recall:
            row["score"] = 0.5
        almost_no_recall[-1]["score"] = 0.9
        self.assertEqual(tool.calibrate(almost_no_recall)["reason"], "selection_quality")

    def test_human_only_labels_missing_is_unknown_and_conflicts_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "labels.csv"
            path.write_text("ImageID,Source,LabelName,Confidence\n"
                            "a,verification,cat,1.0\nb,crowdsource-verification,cat,0\n"
                            "c,machine,cat,1\nd,verification,dog,1\n")
            self.assertEqual(tool.annotations(path, {"cat"}), {"cat": {"a": 1, "b": 0}})
            with path.open("a") as stream:
                stream.write("a,verification,cat,0\n")
            with self.assertRaisesRegex(ValueError, "contradictory"):
                tool.annotations(path, {"cat"})

    def test_vectors_and_model_scoring(self):
        for vector in ([float("nan")], [float("inf")], [0], [1, 0]):
            with self.assertRaises(ValueError):
                tool.validate_vector(vector, 1)
        self.assertEqual(tool.validate_vector([1], 1), [1])
        self.assertAlmostEqual(tool.probability([1], [1], {"logit_scale": 2, "logit_bias": -2}), 0.5)

    def test_full_command_fixture_resume_and_hash_refusal(self):
        info = {"model_id": "fixture", "embedding_dim": 768, "revision": "fixture",
                "precision": "fixture", "image_size": 224, "text_max_tokens": 64,
                "logit_scale": 10, "logit_bias": -5, "runtime": "fixture",
                "tagger_version": "fixture", "device": "fixture"}

        class Sidecar(http.server.BaseHTTPRequestHandler):
            calls = 0

            def log_message(self, *_args):
                pass

            def send(self, value):
                body = json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self):
                self.send(info)

            def do_POST(self):
                data = self.rfile.read(int(self.headers["Content-Length"]))
                vector = [1, 0] + [0] * 766
                if self.path.endswith("text"):
                    self.send({"model_id": "fixture", "embeddings": [vector]})
                else:
                    Sidecar.calls += 1
                    self.send({"model_id": "fixture", "embedding": vector if data == b"1" else [0, 1] + [0] * 766})

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Sidecar)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                images = root / "source-images"
                images.mkdir()
                labels = root / "labels.csv"
                with labels.open("w", newline="") as stream:
                    writer = csv.writer(stream)
                    writer.writerow(["ImageID", "Source", "LabelName", "Confidence"])
                    for i in range(40):
                        image_id = f"{i:016x}"
                        (images / f"{image_id}.jpg").write_text(str(i % 2))
                        writer.writerow([image_id, "verification", "/cat", i % 2])
                classes = root / "classes.csv"
                classes.write_text("/cat,Cat\n")
                lock = {"dataset": "synthetic-test-only", "split": "fixture",
                        "images": images.as_uri() + "/{image_id}.jpg"}
                for name, path in [("labels", labels), ("classes", classes)]:
                    lock[name] = {"url": path.as_uri(), "sha256": tool.digest(path.read_bytes())}
                tool.write_json(root / "lock.json", lock)
                tool.write_json(root / "catalog.json", {
                    "schema_version": 1, "version": "fixture", "model_id": "fixture",
                    "prompt_template": "a photo of {label}", "concepts": [{
                        "id": "cat", "label_en": "cat", "label_fr": "chat",
                        "open_images": "Cat", "parents": [], "threshold": None}]})
                args = argparse.Namespace(catalog=root / "catalog.json", dataset_lock=root / "lock.json",
                    cache=root / "cache", output=root / "output", deadline=60,
                    tagger=f"http://127.0.0.1:{server.server_port}", sample_per_concept=40,
                    precision=0.9, recall=0.1, minimum=5)
                tool.run(args)
                report = (args.output / "calibration-report.json").read_bytes()
                self.assertEqual(tool.read_json(args.output / "calibration-report.json")["concepts"]["cat"]["status"], "calibrated")
                args.catalog = args.output / "concepts.json"
                tool.run(args)
                self.assertEqual(Sidecar.calls, 40)
                self.assertEqual(report, (args.output / "calibration-report.json").read_bytes())
                info["model_id"] = "other-model"
                with self.assertRaisesRegex(ValueError, "pinned model"):
                    tool.run(args)
                info["model_id"] = "fixture"
                (args.cache / "labels.csv").write_text("tampered")
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    tool.run(args)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


class CommittedArtifactsTests(unittest.TestCase):
    def test_catalog_report_tool_and_dataset_are_one_release(self):
        catalog = tool.read_json(tool.ROOT / "gallery-backend/assets/concepts.json")
        report = tool.read_json(Path(tool.__file__).with_name("calibration-report.json"))
        self.assertEqual(report["tool_sha256"], tool.digest(Path(tool.__file__).read_bytes()))
        self.assertEqual(report["dataset"], tool.read_json(Path(tool.__file__).with_name("dataset.lock.json")))
        definitions = {**catalog, "concepts": [{k: v for k, v in c.items() if k != "threshold"}
                                              for c in catalog["concepts"]]}
        self.assertEqual(report["catalog_definition_sha256"], tool.digest(json.dumps(definitions, sort_keys=True).encode()))
        self.assertEqual(catalog["model_id"], report["model"]["model_id"])
        self.assertEqual(catalog["version"], report["vocabulary_version"])
        self.assertEqual(set(report["concepts"]), {c["id"] for c in catalog["concepts"]})
        for concept in catalog["concepts"]:
            result = report["concepts"][concept["id"]]
            self.assertEqual(concept["threshold"], result["threshold"])
            if result["status"] == "calibrated":
                self.assertTrue(0 < result["threshold"] < 1)
                for partition in ("selection", "evaluation"):
                    self.assertTrue(tool.enough(result[partition], report["method"]["minimum_per_class_per_partition"]))
                    self.assertTrue(tool.passes(result[partition], report["method"]["target_precision"],
                                               report["method"]["minimum_recall"],
                                               report["method"]["minimum_predictions_per_partition"]))
            else:
                self.assertEqual(result["status"], "dropped")
                self.assertIsNone(result["threshold"])
                self.assertTrue(result["reason"])


if __name__ == "__main__":
    unittest.main()
