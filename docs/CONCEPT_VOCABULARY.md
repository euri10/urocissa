# Built-in Concept vocabulary

Owner: `infra-urocissa-autotag-sjfz.3`. Definitions live in
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

For each Concept, take the first 160 human-labelled photos ordered by
SHA-256(`urocissa-concepts-v1:ImageID`), or all labelled photos if fewer exist.
This samples the labelled pool uniformly without balancing classes or looking
at scores. Partition each photo independently by the first byte of
SHA-256(`split:ImageID`): below 128 selects thresholds; the rest evaluates them.
A photo can never cross partitions, even when several Concepts name it.
Every image is fetched from the official CVDF mirror; its SHA-256 is retained
in the local manifest. That manifest's canonical digest appears in the report.
Image embeddings are cached against model/runtime/device identity and image
bytes. Both startup and final sidecar identity must match.

For each Concept, choose the threshold with maximum selection recall at
precision at least 0.90, recall at least 0.10 and at least five predictions.
Both partitions must have at least five explicit positives and five explicit
negatives. Tied scores are evaluated together. Evaluate the chosen threshold
unchanged on the other partition; keep it only if the same gates pass.
Otherwise retain a `null` threshold and a report reason. The report includes
positive/negative support, predictions, true positives, precision and recall
for every Concept, including failures. It never silently substitutes 0.5.

This is a bounded first calibration, conditional on Open Images' labelled
pools. Small samples and dataset shift limit its precision estimates; a 90%
sample precision is not a statistical guarantee for the owner's library.
The epic's independent approximately 100-photo owner review, target precision
85%, remains a production acceptance gate. Rare Concepts may need more
human-labelled data. Do not relax the thresholds to make a preferred word fire.
Semantic search and personal learning are the recall paths.

### Recorded calibration (2026-11-08)

The run processed **7,035 public photos** for 72 candidate Concepts. **28**
passed both partitions; their held-out precision ranges from **90.2% to 100%**
and recall from **26.5% to 100%**. Passing examples include personne, enfant,
foule, portrait, chat, oiseau, montagne, fleur, train and cyclisme.
The image cache, manifest and inference responses remain local; only aggregate
measurements are committed. A subsequent run using the generated catalog and
the same cache reproduced both artifacts byte for byte.

The **44 inactive Concepts** comprise 11 with insufficient sampled support,
8 unable to meet the selection gate, and 25 failing evaluation at the selected
threshold. Insufficient support includes stork and dog: it does not prove
the model cannot recognize them. The sampled positives/negatives are recorded
for each partition; raising support requires more verified data, not a guessed
threshold. Stork remains a named Concept and a child of bird for User tag
aliases and parent search. No failing Concept receives a default activation
threshold. The owner approved the French-label sample on 2026-11-08.

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
