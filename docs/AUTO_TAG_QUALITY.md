# Bounded Auto tag quality changes, 2026-11-24

The broad coverage goal is approximately six to eight meaningful tags on typical
photos, without filler. Tag count, no-tag fraction and correctness are separate
measurements. Public class-enriched tests cannot establish the owner's precision.

## Bee zero-shot quarantine, 2026-11-25

The bee zero-shot fallback is inactive after it fired on three unrelated public
cat and dog portraits. Its original stored calibration threshold and owner-fitted
classifier remain unchanged; User tags, rejections, opt-outs and aliases still
take precedence. The version advances to `-quarantine-2`, rematerializing cached
decisions. This sacrifices untrained bee recall until the fallback is separately
qualified; it does not claim an owner precision result.

An untouched, separately frozen 384-photo Open Images V7 test used the pinned
production tagger on CPU and native materialization. The evaluating agent judged
85 correct, three incorrect and four uncertain of 92 displayed chips on 90 photos:
92.39% correct of all chips, above the unchanged 90%/50-chip screen. All three
incorrect chips were pre-existing `insect` decisions on spiders or a centipede,
tracked separately as `infra-urocissa-autotag-sjfz.32`. Comparing identical inputs
before and after found no other removed or wrong new chips: one `insect` parent
became visible on a real bee when its more specific bee chip was quarantined.
There were no displayed bee chips, so this screen cannot establish bee precision
or recall. These are agent judgments, not independent-human or owner judgments.

The ordered-ID checksum is
`3feea07a8cba82ca3ca2b2d3dfb2794276890df216686254364da994e3896206`;
public prediction and review checksums are
`757a5131d6f2487ddb5cb02586ce572b70b1416e101180d41d959d41e6cc2ef0` and
`9bb72cf6595d2c93c02307ae424c6e0309c6274b830cd53bfed3d3214d8d8edb`.
The public-only fixture regression failed on the three portraits before the
quarantine and passed afterward. It accepts an explicit local scratch fixture;
no raw model output or owner data is published.

## Reference fallback quarantine, 2026-11-25

The five inactive-only public-reference heads are quarantined: `bookcase`,
`fence`, `road`, `rose` and `snow` no longer emit Auto tags when the owner has
no fitted matcher. Their frozen weights, original thresholds and the original
per-class test report remain unchanged as historical evidence, not active
quality claims. Other calibrated thresholds and the owner's learned/similarity
matchers, authored tags, rejections and opt-outs are unchanged. The cache
version gains `-quarantine-1` so existing embeddings rematerialize with the new
policy. No image model or database format changes. This deliberately sacrifices
untrained coverage until representative negatives support requalification;
it does not address the six-to-eight-tags goal.

The original five heads were class-balanced on explicitly verified positive and
negative images. On a representative public development sample their scores
frequently exceeded their cuts on visually unrelated pictures. The wrapper used
the reference decision whenever the owner's base matcher was inactive without
requiring the independent text-prompt decision to agree. That supports a negative
selection/distribution mismatch; prevalence, the precise cause within the learned
weights, production GPU parity and personal-library accuracy remain unproven.
The previously released eight-class gates below are unchanged but are not proof
of random-photo precision. No threshold was lowered or replaced.

The first independently frozen 384-photo test of a four-head quarantine failed:
95 correct, seven incorrect and seven uncertain of 109 displayed chips (87.16%
conservative precision). All three `rose` chips were unrelated, so that candidate
was rejected and `rose` was quarantined too. The separate old `bee` zero-shot
path produced three incorrect and two correct chips; it remains tracked as
`infra-urocissa-autotag-sjfz.31`, not silently changed here.

A second, newly frozen 384-photo Open Images V7 test excluded the earlier
samples, selected IDs before inference, and used the exact pinned tagger CPU
image with native Rust materialization. Agent visual review counted 106 correct,
one incorrect and three uncertain among 110 displayed chips: 96.36% correct of
all chips, above the unchanged 90%/50-prediction screen. The remaining 274 photos
had no chips; mean 0.286, median 0. These are evaluating-agent judgments, not
independent-human or owner precision. Per Concept, correct/incorrect/uncertain:

| Concept | Correct | Incorrect | Uncertain |
| --- | ---: | ---: | ---: |
| animal | 21 | 0 | 1 |
| bird | 10 | 1 | 0 |
| cat | 2 | 0 | 0 |
| dog | 10 | 0 | 0 |
| fish | 4 | 0 | 0 |
| food | 46 | 0 | 1 |
| fruit | 7 | 0 | 1 |
| house | 1 | 0 | 0 |
| insect | 1 | 0 | 0 |
| mushroom | 1 | 0 | 0 |
| portrait | 2 | 0 | 0 |
| skiing | 1 | 0 | 0 |

Concepts absent from this sample have no measured precision or recall. The
ordered-ID checksum is
`df76ec8460d09fee654457e58e4511708544206ee33cc4663481bb4d1e5a66c1`;
the public predictions and visual judgments have SHA-256
`b482e7572949138217b8ebb1a8fafce64216c9ef6101e7938671967d600681a9` and
`6d597e8640e29de3e015e77bec8c294dfde6f7a26efd5536c63a877981caeef4`.
The reproducible native materialization test accepts an explicit public scratch
fixture and writes public predictions only to an explicitly chosen scratch path.
The previous held-out replay still checks all original per-Concept gates using
the historical heads; it does not reactivate them.

The existing owner random round stores its displayed labels at freeze time and
contains 18 `fence` labels, with no judgments recorded at the last read-only
check. It remains intact and can judge the older release, not the corrected
runtime. An owner-chosen new random round after deployment would measure the
corrected release; no re-freeze or review-flow change is performed here. The
existing UI does not expose a reset button, so any replacement of the current
round needs a separately agreed owner-directed operation. Garden was not frozen
at the last check. Personal photos and raw private outputs remain on the host.

## Stronger-model round and a fallback warning, 2026-11-25

The sections below document the original pre-quarantine release and its evidence.

The pending owner review is separate and remains frozen. A 2026-11-25 read-only
recheck matched the earlier fixed private 100-photo sample: 78 photos had no
displayed Auto tags, 19 had one and three had two, for a mean of 0.25. It was
**not** an owner precision judgment. This round made no production model,
feedback, review, or persistent-data changes.

On 64 hash-ranked Open Images V7 test photos, excluding the earlier fixed public
coverage sample, the following frozen, no-feedback candidates were independently
screened against the public annotations and visually inspected by the evaluating
agent. The visual inspection is not an independent human or owner review. Each
prediction was classified as clearly correct, incorrect, or uncertain; the
conservative precision divides correct by **all** predictions, including uncertain
ones. It is not an estimate of owner-library precision.

| Public candidate | Mean tags/photo | Correct / incorrect / uncertain | Conservative precision |
| --- | ---: | ---: | ---: |
| Florence-2-large-ft INT8, expanded reviewed nouns | 2.98 | 146 / 14 / 31 | 76.4% |
| Florence-2-large-ft INT8, existing reviewed nouns | 2.28 | 114 / 8 / 24 | 78.1% |
| RAM++ INT8, expanded reviewed nouns | 1.91 | 94 / 7 / 21 | 77.0% |
| RAM++ INT8, existing reviewed nouns | 1.48 | 79 / 3 / 13 | 83.2% |

The pre-registered public screen required at least 50 predictions and 90%
conservative precision. All four configurations failed; no threshold was lowered
and no candidate was shipped. The original per-Concept precision, equal-prior
precision, recall and support gates, and the owner's >=85% review requirement,
remain separate and unchanged. On this set the official exact-name annotations
for the expanded Florence result were 86 positive, four negative and 101 unknown;
for expanded RAM++ they were 70 positive, three negative and 49 unknown. Missing
annotations are not negatives or proof of correctness.

The Florence pilot used deterministic object detection, dense region captions
and detailed captions. The pinned INT8 large-ft conversion was tested; the
owner-requested large model was also piloted, and AutoTagger was found to wrap
large-ft object detection. A grammar-constrained local Qwen3-VL-2B pilot produced
4.94 tags/photo but only 49 of 79 suggestions were clearly supported, with five
incorrect and 25 uncertain; valid JSON did not prevent hallucination. This small
validation pilot was not a held-out quality or owner-acceptance result.

On beesslc's four-thread Intel N100, five public validation images at two CPU
cores took a median 25.40 seconds each for Florence object detection **alone**,
or about 45.2 hours extrapolated to 6,400 images before other tasks or overhead.
RAM++ took a median 5.07 seconds, about nine hours, and peaked at 5,224 MiB RSS,
above the present 4 GiB worker limit. Five samples do not establish sustained
service impact. Neither model was accepted for production on this evidence.

An additional public no-feedback baseline exposed a quality risk in existing
reference fallbacks. In the same 64-photo public sample, the pinned SigLIP model
and current defaults produced 58 chips: 19 clearly correct, 34 incorrect and
five uncertain in the agent's visual review. The `fence` head fired 27 times,
with two clearly correct, 23 incorrect and two uncertain; `road` fired 11 times,
with one clearly correct, seven incorrect and three uncertain. Native Rust
replay confirmed that the fallback displays `fence` on a public dog portrait
and `fence` and `road` on a public camera close-up. This is a CPU no-feedback
reproduction, **not** a measurement of the owner's trained matchers or GPU
results. Track the unresolved defect as `infra-urocissa-autotag-sjfz.30`; do
not interpret earlier class-enriched gate passes below as a random-photo
precision guarantee.

For future public-only evaluations, `tools/concepts/visual_precision.py` reports
coverage and conservative precision without emitting photo IDs. It requires an
explicit judgment for every predicted label and enforces this round's fixed
50-prediction/90% aggregate screen. Its inputs are a prediction list of
`{"id": "PUBLIC_ID", "labels": ["..."]}` objects and a review object mapping
each public ID to every predicted label and one of `correct`, `incorrect` or
`uncertain`. Empty-label photos require an empty review object. Keep public
images, IDs, predictions and judgments in an explicit local scratch directory;
the output contains aggregates only. Passing this screen alone never authorizes
deployment or substitutes for the per-Concept and owner gates.

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
