# Public-fixture prompt comparison

These five attributed repository photos are a small diagnostic set, not a
representative quality evaluation. Do not substitute private images. The
comparison loads the pinned local worker model offline, uses the same reviewed
label filter and prints raw model output for these public fixtures only.

On 2026-11-22, a Ryzen workstation, 2 CPUs / 4 GiB, 4 model threads, one image
patch, deterministic generation, and the deployed worker image produced:

| Photo | Deployed prompt | Candidate prompt |
| --- | --- | --- |
| bicycles | no labels (model wrote sentences) | bicycle, person, road, water, tree, sky, cloud |
| cat | cat, window | cat, window |
| stork pair | bird nest, tree, sky | bird, nest, tree, sky |
| stork fledglings | bird, tree, sky | bird, nest, tree, sky |
| sunset | rock, water, person, sky, cloud | rock, person, water, beach |

The candidate recovers the visible bicycle and people but also proposes water,
which is not apparent in that photo. The cat fixture's window and the fledgling
fixture's sky are also not clearly grounded. No claim of general precision,
production latency, or correctness on the owner's bridge photo follows. The
candidate can still produce prose or wrong labels; explicit owner review remains
essential. This comparison does not alter the allowlist or add prose extraction.

Run from this repository with Docker and the already-available exact worker
image; no network, credentials, production data or model downloads are used:

```sh
timeout 180s docker run --rm --network none --read-only --tmpfs /tmp:size=128m,mode=1777,noexec,nosuid \
  --cpus 2 --memory 4g --pids-limit 128 --cap-drop ALL --security-opt no-new-privileges:true \
  --user 1000:1000 --mount type=bind,src="$PWD",dst=/src,readonly --entrypoint python \
  registry.gitlab.bartab.fr/infra/urocissa/concept-worker@sha256:a78b2667543a788afdbb616aa4218bdceb4f192e1ce6c4a4ec9f413912861c2d \
  -B /src/benchmarks/concept-suggestions/public_prompt_comparison.py
```

The opt-in real-model regression is
`test_local_vlm_worker.LocalVlmWorkerTests.test_real_model_returns_useful_labels_for_public_bicycle_photo`.
Run it with the same isolated image and bind mount, working directory
`/src/tools/concepts`, and environment variable
`UROCISSA_CONCEPT_MODEL_DIR=/opt/model`. It failed before the prompt change and
passed afterwards. Normal unit tests skip it when the local model is absent.
