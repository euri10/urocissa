# Built-in Concept vocabulary

Owners: `infra-urocissa-autotag-sjfz.3` and calibration fix `.16`. Definitions live in
`gallery-backend/assets/concepts.json`; the aggregate measurements are in
`tools/concepts/calibration-report.json`. This source work does not activate
Auto tags or backfill on the production library.

Each Concept has a stable ID, English scoring label, French display label,
optional aliases, parent IDs and a calibrated probability threshold. The whole
file has a vocabulary version and an exact model ID. Prompts use
`a photo of {label}`. The gallery must cache their vectors by **both** model ID
and vocabulary version. A model or prompt change requires new calibration;
thresholds from another model must never be reused.

`BuiltinCatalog::embedded()` validates the file with the existing engine's
unique-name, threshold and acyclic-hierarchy checks. `into_engine()` requires
one valid normalized vector per Concept and the matching model ID, and installs
the activation gate. A `null` threshold means the Concept failed calibration or
has insufficient evidence: it cannot fire from scoring. Its name and hierarchy
remain available to aliases and parent searches. Never build a default
zero-shot engine directly from these definitions, or replace the activation
gate when adding learned classifiers without preserving it for unlearned
Concepts. The subsequent learning slice owns that composition.

The explicit reviewed ID/English-label allow-list in `builtin.rs` enforces the
epic's exclusion policy. It includes neutral person, crowd, portrait, selfie,
baby and child, plus objects, scenes, animals, activities and events (including
protest, banner, police and stork). It excludes inferred gender, ethnicity,
religion, orientation, disability, emotions and body/attractiveness attributes.
New built-in IDs require a policy review and test update; personal User tags do
not go through this built-in list.

## Reproduce calibration

Use an already-running **stateless** gallery-tagger built from the pinned source
and weights. The recorded run uses the local amd64 image
`sha256:f485e8499bd5ec406e96bc5a9c90978b4c8b0e9856c633d294e2abed2a87f1df`
at CPU f32 precision with two threads. Check the exact image ID against your
locally retained image before using it. Its registry manifest digest is
`sha256:47e4e31cd43f7a9881ea99e79f742a1efec3aa86864129b3931526cc32e8b7a0`.
The run never sends private library images to the sidecar.

From the application repository:

```sh
python3 -B -m unittest discover -s tools/concepts -v
python3 -B tools/concepts/calibrate.py \
  --cache /ABSOLUTE/SCRATCH/openimages-concepts \
  --output /ABSOLUTE/SCRATCH/calibrated-concepts \
  --tagger http://127.0.0.1:15674 --deadline 7200
```

The command intentionally downloads public Open Images data. It installs no
Python packages, starts no services and touches no gallery database. Keep the
cache and output outside the repository. After reviewing the report, promote
the generated `concepts.json` and `calibration-report.json` to their versioned
locations together. Thresholds are outputs, so the report hashes the canonical
input definitions without thresholds; rerunning the generated catalog can
reproduce the same report. Increment the vocabulary version before changing
definitions, model, prompts, sampling or accepted thresholds.

`dataset.lock.json` pins the exact Open Images V7 validation human-label and
class-description bytes with SHA-256. The command refuses different metadata,
fractional human labels and contradictory annotations. Only `verification`
and `crowdsource-verification` rows count. Machine labels and unknown labels
are ignored. Open Images annotations are non-exhaustive; an absent row is
**not** a negative example. See the [official label format](https://storage.googleapis.com/openimages/web/download_v7.html#image-level-annotations)
and [evaluation policy](https://storage.googleapis.com/openimages/web/evaluation.html).

For each Concept, take at most 80 explicit positives and 80 explicit negatives,
each ordered by SHA-256(`urocissa-concepts-v1:ImageID`). Never refill a scarce
class from the majority, invent a negative from an unknown label, or use model
scores to choose the sample. `--sample-per-concept` is an even total budget;
half is reserved for each verified class. Partition each photo independently by the first byte of
SHA-256(`split:ImageID`): below 128 selects thresholds; the rest evaluates them.
A photo can never cross partitions, even when several Concepts name it.
Every image is fetched from the official CVDF mirror; its SHA-256 is retained
in the local manifest. That manifest's canonical digest appears in the report.
Image embeddings are cached against model/runtime/device identity and image
bytes. Both startup and final sidecar identity must match.

For each Concept, choose the threshold with maximum selection recall at
both observed precision and **equal-prior precision** at least 0.90, recall at
least 0.10 and at least five predictions.
Both partitions must have at least five explicit positives and five explicit
negatives. Tied scores are evaluated together. Evaluate the chosen threshold
unchanged on the other partition; keep it only if the same gates pass.
Otherwise retain a `null` threshold and a report reason. The report includes
positive/negative support, predictions, true positives, precision and recall
for every Concept, including failures. It never silently substitutes 0.5.

Equal-prior precision is `TPR / (TPR + FPR)`, where TPR is recall and FPR is
false positives / explicit negatives. It weights the two classes equally rather
than inheriting the annotation pool's prevalence. Without both classes or
predictions it is zero. An always-on classifier scores 0.5, even in a pool with
95% positives. The report (schema 2) records both precision measures and FPR.
Selection maximizes recall, then equal-prior precision, observed precision and
threshold; the evaluation partition never tunes that choice.

This is a bounded first calibration, conditional on Open Images' labelled
pools. Small samples and dataset shift limit its precision estimates; a 90%
sample precision is not a statistical guarantee for the owner's library.
The epic's independent approximately 100-photo owner review, target precision
85%, remains a production acceptance gate. Rare Concepts may need more
human-labelled data. Do not relax the thresholds to make a preferred word fire.
Semantic search and personal learning are the recall paths.

### Recorded calibration (2026-11-08)

Vocabulary **2026-11-08.2** processes **6,601 public photos** for 72 candidate
Concepts, with 606 additional downloads beyond the previous cache. **21** pass
both partitions: observed held-out precision ranges from **91.3% to 100%**,
equal-prior precision from **90.6% to 100%**, and recall from **23.4% to 96.6%**.
Passing examples include animal, foule, chat, chien, oiseau, fleur, portrait
and train.
The image cache, manifest and inference responses remain local; only aggregate
measurements are committed. A subsequent run using the generated catalog and
the same cache reproduced both artifacts byte for byte.

The **51 inactive Concepts** comprise 10 with insufficient sampled support,
11 unable to meet the selection gate, and 30 failing evaluation at the selected
threshold. Insufficient support includes stork: it does not prove
the model cannot recognize them. The sampled positives/negatives are recorded
for each partition; raising support requires more verified data, not a guessed
threshold. Stork remains a named Concept and a child of bird for User tag
aliases and parent search. No failing Concept receives a default activation
threshold. The owner approved the French-label sample on 2026-11-08.

This replaces the first 7,035-image, 28-active-Concept release. Its positive-
enriched Cloud pool had 75 positives / 6 negatives in selection: predicting
Cloud for **all 81** still passed observed precision (92.6%). The similarly
skewed evaluation pool also passed. This made even blank images and a cat fire
Cloud; Snow also fired on the cat. Balanced sampling plus both precision gates
fix the calibration rule, not those individual scores. Cloud now fails selection;
Snow fails independent evaluation. Both remain inactive until adequate evidence
passes the unchanged quality targets. French labels and model identity are unchanged.

Nine independently reviewed public/synthetic images are also checked outside
threshold selection. `check_fixtures.py` records exact-sidecar scores in
`fixture-scores.json`, binding model, prompts and source-image SHA-256. It
checks only explicitly reviewed absences (others are unknown), plus positive
controls requiring Cat on the cat and Bird on both stork fixtures. These scores
never enter the calibration sample or threshold selection. All checks pass for
this release; they detected six failures in the previous one.

```sh
python3 -B tools/concepts/check_fixtures.py \
  --tagger http://127.0.0.1:15674 --output /ABSOLUTE/SCRATCH/fixture-scores.json
```

Fresh probes reproduced the captured scores byte for byte. The stork photos,
bicycle tour, sunset, screenshot and three synthetic images remain attributed
in `gallery-tagger/tests/fixtures/ATTRIBUTION.md`. The CI fixture also checks
their hashes, and runs when either the calibration files or source images change.

Local acceptance: 12 Python calibration/provenance/regression tests and 314
backend tests pass (3 intentionally ignored). A source-built gallery opened a
disposable copy containing nine already-embedded fixture photos and rematerialized
them for the new vocabulary version. Actual batch and direct owner responses retain
Cat on the cat and Bird on both storks, with no Cloud/Snow. Anonymous responses
still omit Auto tags. No frontend or authored-record schema changed.

Remaining quality issue `infra-urocissa-autotag-sjfz.17`: the cat still triggers
Flower (score 0.0039003, threshold 0.00117167), despite Flower passing both
sampled precision gates. This separate false positive is not covered by the
Cloud/Snow absence assertions or claimed fixed here. Sampled metrics and these
nine fixtures do not replace the production owner review.

The Python fixture in CI exercises download/hash checks, the sidecar HTTP
contract, threshold selection, held-out rejection and cache resume with
synthetic data. It is not model-quality evidence. Rust tests additionally cover
schema/policy refusal, French labels, model/vector mismatch, inactive Concepts
and hierarchy expansion. No frontend changed; the opt-in hybrid-scroll gate
is not required for this work.

## Sources and licences

The 72 Concept choices, French translations, stable IDs and hierarchy were
authored for this epic under the repository's licence, using the owner's
confirmed scope and common photo subjects. English mapping names come from
Open Images' class descriptions; no third-party vocabulary package was copied.
Neither the labels CSV nor calibration photos are shipped in the repository or
application image. The existing SigLIP 2 model/export licence is documented in
`gallery-tagger/README.md` and its pinned model lock.

Google LLC releases the Open Images annotations under **CC BY 4.0**. Attribution:
Alina Kuznetsova et al., *The Open Images Dataset V4: Unified image
classification, object detection, and visual relationship detection at scale*,
IJCV, 2020; the [Open Images V7 project](https://storage.googleapis.com/openimages/web/factsfigures_v7.html)
requests that reference for V5–V7 as well. Class mappings and calibration
aggregates here are adaptations; source bytes are identified in the lock.

Open Images lists the photographs as **CC BY 2.0**, with a warning that individual
licences must be checked. The CVDF mirror serves rescaled images, at most
1024 pixels on the longest side. They are used locally for calibration only;
this work redistributes no photographs. Consult the
[official licence statement](https://storage.googleapis.com/openimages/web/factsfigures_v7.html#licenses)
and [CVDF image source](https://github.com/cvdfoundation/open-images-dataset)
before publishing any calibration images or visual examples.
