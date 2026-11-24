# Bounded Auto tag quality changes, 2026-11-24

The broad coverage goal is approximately six to eight meaningful tags on typical
photos, without filler. Tag count, no-tag fraction and correctness are separate
measurements. Public class-enriched tests cannot establish the owner's precision.

## Snow feedback classifier

Only `snow` gains feedback-only logistic fitting after at least five distinct
positive and five genuine rejected current-embedding examples. Before that gate
it uses the existing example matcher. The logistic implementation, 0.8 decision
threshold and balanced training are already used by calibrated Concepts; snow
uses a zero prompt feature because its visual prompt failed calibration.
Other uncalibrated Concepts remain on their prior matching policy. Authored tags,
per-photo opt-outs and permanent Rejections retain the engine's precedence.

This bounded exception follows a preregistered public experiment. For each seed
0–4, select 30 explicit positives and six explicit negatives from the existing
Open Images validation **selection** partition, ranked by
SHA256(`urocissa-snow-feedback-v1:SEED:ImageID`). Run the actual Rust `Examples`
and `Logistic` implementations on the same embeddings, with no threshold tuning.
Evaluate the untouched validation evaluation partition (39 positive, 43 negative)
and an independent Open Images test set (80 positive, 80 negative), selected by
SHA256(`urocissa-everyday-test-v1:ImageID`) within each verified class, no refill.

| Seed | Validation old TP/FP | Validation new TP/FP | Independent old TP/FP | Independent new TP/FP |
| --- | --- | --- | --- | --- |
| 0 | 23/9 | 28/0 | 55/20 | 57/3 |
| 1 | 22/6 | 24/0 | 45/9 | 45/0 |
| 2 | 18/4 | 24/0 | 47/11 | 49/1 |
| 3 | 19/6 | 31/1 | 41/9 | 57/1 |
| 4 | 9/2 | 24/0 | 27/3 | 42/1 |

Every new trial passes the unchanged gates: at least 90% observed and
equal-prior precision, 10% recall and five predictions with five examples per
class. Trials share evaluation images; do not pool them as independent evidence.
Independent-test precision ranges from 95% to 100%, recall from 52.5% to 71.25%.
Official labels remain unchanged, including any ambiguity. Model pretraining
overlap and transfer to owner photos are unknown.

Model identity: `siglip2-base-patch16-224@75de2d55ec2d/fp16-text-int8`, OpenVINO
2026.4.1 CPU/f32. Independent annotation SHA-256:
`917d3ca8c0a2e09d652fb3148e9264df6117bfcaccf425a60a29b23269487953`.
Public input fixture SHA-256:
`bb37fa0881a375182c6d4fc341947d0ede788922bb6630ae930e66c02d346723`.
Extracted unchanged Rust learner body SHA-256:
`33df9d9a70ff955e28df14b69e52ae6419ec8822002be66e1f42f55363f0a419`.
Sample IDs/directions/embeddings and public images remain in the explicit local
public-data scratch; no owner photos or feedback were used in this experiment.

The explicit native replay is available without copying the fixture into Git:

```sh
UROCISSA_PUBLIC_FEEDBACK_FIXTURE=/ABSOLUTE/PUBLIC-SCRATCH/feedback/inputs.json \
  cargo test --locked --offline public_snow_feedback_quality -- --ignored --nocapture
```

Verify the fixture SHA above first. The replay asserts every fixed quality gate
using the current Rust implementation and emits only public aggregate counts.

The change refits from existing authored feedback and cached embeddings during
ordinary startup rematerialization. It changes no V6 record format, authored
table or derived classifier encoding. Rollback redeploys the retained image/config
and refits the earlier matcher, preserving the external volume. A fresh checked
backup and application readiness are still required for deployment.

The specific waterside photo still needs owner judgement. The consolidated
acceptance round must keep that subjective result separate from public metrics.
The unsafe global switch rejected for building/boat remains excluded.

## Wider vocabulary and diagnosis

Vocabulary promotion also preserves Rejections authored before a name became
built-in. The exact normalized label and declared synonyms resolve their old
`personal:` identities to the current Concept. Readback, negative-example counts,
prediction and conflict checks use one canonical decision. New rejection writes
retain current and historical keys in the existing table so the previous binary
can honor them; explicit Undo clears those equivalent keys only. Unrelated
personal decisions and V6 records remain intact, with no startup migration.
Tracking: `infra-urocissa-autotag-sjfz.28.3`.

`tools/concepts/heldout.py` evaluates frozen new thresholds against the pinned
independent test annotations and measures both baseline and candidate chip-count
distributions on 100 fixed hash-ranked public test photos. Missing annotations
remain unknown; they cannot establish precision. Existing ancestor suppression
and the eight-chip cap apply. The pre-registered screen uses the existing
90% observed/equal-prior, 10% recall and five-support/prediction gates unchanged.

[Owner-only per-photo explanations](AUTO_TAG_DIAGNOSTICS.md) distinguish scoring,
feedback and inherited parent placements without inference, feedback changes or
database copies. They provide evidence for the open building and bridge findings;
diagnosis alone does not resolve those quality findings.

## Frozen public defaults, 2026-11-24

`assets/concept-defaults.json` is an additive, exact-model supplement to the
original 75-Concept catalog. The original catalog, calibration report, fixture
scores and Flower quarantine remain unchanged. Seven neutral names are added:
bench, bookcase, fence, insect, road, rose and spider. Bee/butterfly now refine
insect; rose refines flower. A parent implied by a valid descendant remains
searchable, not an extra displayed chip. Flower itself stays quarantined.

Three additions use independently qualified zero-shot thresholds: bench, insect,
spider. Five use frozen public-reference linear heads: bookcase, fence, road,
rose, snow. The latter reuse the current SigLIP2 image vector; there is no extra
pixel model or inference service. A reference head runs **only** while the existing
per-Concept matcher is inactive. Existing calibrated zero-shot, owner similarity
and owner learned classifiers take precedence. Rejections, historical personal
Rejections, authored aliases, hierarchy, opt-out and the eight-chip cap still apply.
The UI correctly calls these defaults built-in visual detection.

The screen considered 60 additional neutral Open Images names plus inactive
original Concepts. Public reference heads used the unchanged Rust 160-step,
class-balanced Logistic learner (learning rate 2, L2 .002), with the prompt-score
feature fixed to zero. Original selection photos whose
SHA256(`reference-train:ImageID`) first byte is below 128 train the direction;
the remaining selection photos choose its threshold. Original evaluation photos
remain untouched. Every training class needs five positives **and** five genuine
negatives. No missing annotation is a negative.

Six new zero-shot candidates and thirteen public-reference heads passed original
selection/evaluation. Their thresholds were then frozen. A separate official test
split, up to 80 verified positives and 80 negatives per Concept, rejected eleven
of nineteen candidates. No failed threshold was retuned or replaced. The remaining
eight also passed the existing reviewed fixture absences and a veto requiring
them absent on the three pre-existing neutral synthetic images.

| Concept | Independent TP / positives | FP / negatives | Equal-prior precision |
| --- | --- | --- | --- |
| bench | 9 / 80 | 0 / 80 | 100% |
| bookcase | 39 / 80 | 0 / 80 | 100% |
| fence | 50 / 80 | 0 / 80 | 100% |
| insect | 64 / 80 | 4 / 80 | 94.1% |
| road | 60 / 80 | 1 / 80 | 98.4% |
| rose | 23 / 80 | 0 / 80 | 100% |
| snow | 58 / 80 | 1 / 80 | 98.3% |
| spider | 63 / 80 | 1 / 51 | 97.6% |

All pass the unchanged >=90% observed/equal-prior precision, >=10% recall,
five examples per class and five predictions gates in each partition. These are
point-estimate gates, not lower confidence bounds. The actual Rust engine replay
produced exactly the same decisions as the public evaluator for all eight classes.
The aggregate provenance and both passing/failed independent results are in
`tools/concepts/public-defaults-report.json`. Public pixels/vectors remain in the
explicit local scratch; no owner examples informed these defaults.

On the fixed, disjoint 100-photo public sample, the qualified release changes:

| Displayed Auto tags | Original photos | Qualified release photos |
| --- | --- | --- |
| 0 | 79 | 33 |
| 1 | 21 | 35 |
| 2 | 0 | 27 |
| 3 | 0 | 5 |
| 4–8 | 0 | 0 |

Mean rises 0.21 -> 1.04, median 0 -> 1. This is **not** the owner's 6–8 goal.
Of 104 candidate chips, sparse official annotations verify 19 positive, 0 negative
and leave 85 unknown; this cannot establish overall precision. Do not use the
earlier all-19 prototype's 2.57 mean: eleven of those candidates failed and are not
released. Owner-library counts and subjective precision remain separate acceptance
checks; `.28` remains open. Building and bridge matchers are unchanged because the
tested alternatives failed frozen quality gates.

Reproduce packaging from the retained explicit public scratch (no network or
threshold selection), and run the opt-in native replay:

```sh
python3 -B tools/concepts/package_defaults.py --scratch /ABSOLUTE/PUBLIC-SCRATCH
cd gallery-backend
UROCISSA_PUBLIC_DEFAULTS_FIXTURE=/ABSOLUTE/PUBLIC-SCRATCH/public-defaults-inputs.json \
  cargo test --locked --offline public_default_heldout_quality -- --ignored --nocapture
```

Native public fixture SHA256:
`28a7d2266642b0502c6abbd2ba7ee855a13e4798a73cbe715699dfda40894699`.
The learner source-body SHA remains the one recorded above. The public held-out
tool verifies annotation hashes, model identity and image/cache hashes; bounded
transport retries reuse the exact URL and never substitute failed images.

Startup prepares the seven additional text vectors once, reuses cached photo
embeddings and rematerializes under vocabulary `2026-11-24.1`. This changes no
authored or derived storage schema. Deployment still requires the fresh checked
backup and tested retained-image rollback. Rollback rematerializes the prior
vocabulary from unchanged authored decisions; no production data restoration is
needed for an ordinary binary rollback.
