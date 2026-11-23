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

## Broader public-sample diagnostic, 2026-11-22

This is an exploratory comparison, not owner-library quality acceptance. The
fixed sample is ranks 32–63 of 41,620 Open Images V7 validation images having
human verification, ordered by SHA-256 of `urocissa-coverage-v1:<image-id>`.
It is disjoint from the five repository photos used above, but it is not a
random sample of the owner's library and model-training overlap is unknown.
The public dataset's image-level annotations are incomplete: an absent label
is not proof that an object is absent. Individual image licences must be checked
before redistributing images; no images are included here. See
`docs/CONCEPT_VOCABULARY.md` for dataset attribution and limitations.

Using the deployed worker model `4ccc2be9...`, the current prompt and exact
reviewed-label filter returned no labels for 13/32 images, with 37 returned
labels total. The experimental prompt in `public_sample_comparison.py` returned
no labels for 6/32, with 59 labels total. These are **coverage counts**, not
precision. Visual inspection found the current prompt output `cat` for an SUV
whose number plate reads CAT; the experimental prompt instead returned `car`,
`house`, `tree`. On a boat photo, the experiment returned `boat`, `house`,
`tree`, `grass`, `car`; a car is not visible. Output repeatedly generated
tokens in at least six of 32 cases with each prompt; generation was capped at
96 tokens.
The experiment does not solve hallucinations; this screen alone does not prove
it safe or accurate. The owner subsequently chose to try the increased
coverage as explicitly reviewed suggestions despite the known mistakes. No
owner photo or feedback was used in this public screen.

The raw public outputs, per-photo hashes, filtered labels and local generation
times are in `public-sample-20261122.json`; times are workstation diagnostics,
not production latency. The manifest records the selected image IDs and hashes.
The experiment used a networkless container, 2 CPUs, 4 GiB, 4 PyTorch threads,
one image patch and deterministic generation; preprocessing matches the worker.
The test runs generation directly, without exercising its socket, timeout guard
or gallery thumbnail pipeline. The `deployed` mode is the prompt deployed on
2026-11-22; both comparison strings are frozen so future worker prompt changes
do not rewrite this historical experiment.

To reproduce, obtain only the public validation images identified in
`public-sample-20261122-manifest.json` from the dataset URL in
`tools/concepts/dataset.lock.json`, check individual licences, and put them in
an absolute sample directory named `01-<image-id>.jpg`, etc., with the manifest
copied there as `manifest.json`. The runner checks every file's hash. With the
already available exact worker image and no network/model download:

```sh
docker run --rm --network none --read-only --tmpfs /tmp:size=128m,mode=1777,noexec,nosuid \
  --cpus 2 --memory 4g --pids-limit 128 --cap-drop ALL --security-opt no-new-privileges:true \
  --user 1000:1000 -e HF_HUB_OFFLINE=1 -e TRANSFORMERS_OFFLINE=1 \
  --mount type=bind,src="$PWD",dst=/app,readonly \
  --mount type=bind,src=/ABSOLUTE/PUBLIC_SAMPLE,dst=/sample,readonly \
  --entrypoint python \
  registry.gitlab.bartab.fr/infra/urocissa/concept-worker@sha256:4ccc2be94de12c181ec1a3f3435f041991d91f793b829966376aa578e038ac7b \
  -B /app/benchmarks/concept-suggestions/public_sample_comparison.py /sample deployed
```

Repeat with `experimental` instead of `deployed` for the paired comparison.
This tool prints raw model output: **never use it with private photos**. The
next quality step is independent visual annotation of a larger public holdout,
including false positives and uncertain labels, before choosing a safer model
or prompt. User review and the existing label/privacy boundaries remain required.

## Separate public holdout

After freezing the two prompts, the same procedure was applied to the distinct
32 images at offsets 0–31 of the same verified Open Images population. This set
was used in earlier, unrelated model exploration, so it is not a pristine
project-wide holdout. It is disjoint from the prompt comparison above; model
training overlap is unknown. The manifest and paired raw outputs are saved in
`public-holdout-20261122-manifest.json` and `public-holdout-20261122.json`.

| Measure | Deployed prompt | Experimental prompt |
| --- | ---: | ---: |
| Photos with no returned labels | 24/32 | 16/32 |
| Returned labels | 13 | 37 |
| Apparently correct | 9 | 28 |
| Apparently incorrect | 2 | 2 |
| Uncertain | 2 | 7 |

The label assessments are **unblinded AI visual judgments**, not human-reviewed
ground truth: each unique photo/label pair has the same verdict for both
prompts, and ambiguous items remain uncertain. For example, the deployed
prompt returned carrot and tomato for a photo of coconuts, while the
experimental prompt returned rock for a frog on a branch and bench for a gym
without a visible bench. Conversely, the experimental prompt returned road,
building, tree and sky on a bus photo where the deployed prompt returned no
allowed label. These examples show a plausible coverage gain alongside errors,
not a precision or recall guarantee. The sample is small, incomplete and not
representative of the owner's library; photo-level recall was not annotated.
Human review of these public judgments and a separate representative evaluation
remain necessary before deployment or a claim of quality acceptance. No
personal image, tag, feedback, or production process was touched. The newer
generic prompt is proposed for owner-requested review only, not background
auto-tagging: no suggestion is written until the owner explicitly confirms it.

### Owner's targeted review

For four selected public photos, the owner judged `rock` on the frog incorrect,
`bench` in the gym correct, both `fence` and `house` behind the penguins
incorrect, and `wall` on the dark photo uncertain. These five owner judgments
are stored separately in `owner_review` in the holdout JSON: they correct the
agent's earlier gym-bench judgment and resolve two previously uncertain
labels, without overwriting the original assessment. This targeted selection
cannot establish overall precision. The table above remains the original
provisional, unblinded agent assessment, not a mixed human/agent score.
