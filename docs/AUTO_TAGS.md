# Owner-only Auto tags

## Integrated rollout candidate (.13)

The release combines the completed feature chain through Concept review
(`f7235cfe`) with the evidence-bound Flower quarantine (`c8247af7`). The latter
keeps Flower named but inactive and preserves the reviewed calibration evidence;
it does not adjust thresholds against the independent cat fixture. Storage,
owner feedback, personal Concepts, tag search/facets, semantic search and Similar
photos are integrated in the same source tree.

Combined local validation: 324 backend tests passed (4 opt-in tests ignored),
343 frontend tests passed, and typecheck, lint, production build, Rust formatting
and Clippy passed (existing warnings only). All 12 calibration-tool tests passed.
The actual previously pinned gallery executable from manifest
`sha256:1032dfee0469b6129f31eda6bb8cc63640e55b1bacd8ba97c0e18c50c2511d22`
also opened the new authored database, served confirmed User tags and preserved
Rejections in the explicit rollback-reader test. That test was run with
`UROCISSA_PREVIOUS_BINARY`, rather than relying on its no-binary early return.

Exact-image pair verification and host acceptance belong to the infrastructure
rollout. These local checks are not a production backup, N100 benchmark, live
backfill or the owner's 100-photo quality review. The expensive hybrid-scroll
gate was not requested and was not run.

When the optional tagger is configured, photos receive Auto tags from the reviewed,
calibrated built-in Concept vocabulary. User tags remain authored, filled chips;
Auto tags are separate outlined sparkle chips with keyboard/hover confidence
tooltips. Videos are not auto-tagged. Inactive, uncalibrated Concepts never fire.

The upload dialog's **Auto-tag** switch starts on and remembers its choice in local
storage. **Publish immediately** still resets off whenever the dialog reopens.
Drop/paste intake uses the same remembered default. Each queued upload snapshots
both choices. Owner requests can set
`/upload?autotag=false`; omission means on. Share uploads cannot set this option.
Duplicate uploads preserve the existing photo's preference.

The photo and selection menus can turn Auto tags on/off. Mixed selections ignore
videos and reject albums. Owner read-only mode disables these changes. Turning
tags off hides them immediately, but does not disable embeddings for semantic
search or Similar photos. Turning them back on uses cached vectors, including
while the sidecar is offline.

Each Auto chip has **Confirm** (check) and **Reject** (cross) buttons. Confirmation
writes the Concept's French label into the ordinary V6 User tags. That tag is
visible to visitors on published photos and remains when tagging is disabled or
the gallery binary is rolled back. Rejection permanently suppresses that stable
Concept ID on the photo, across recomputes, vocabulary releases and model changes.
Removing a matching User tag records the same permanent Rejection, including
while tagging is off or offline. Manually adding a User tag remains possible;
it never erases a Rejection or permits the Concept to auto-apply again.

The selection menu's **Confirm or reject Auto tags** dialog applies a chosen
Concept to every selected photo, including ones without a current prediction.
It previews the exact photo count and how many are published before confirmation.
Videos are ignored and albums are rejected. The dialog states that rejection is
permanent. All decisions require writable owner access and use the existing
snapshot generation checks. Loading, read-only and failed requests keep the
controls and feedback explicit.

## Storage and response boundaries

- Main database: `auto_tags_off_v1` stores authored, default-on preferences. It is
  additive; `auto_tag_rejections_v1` stores permanent `(photo, Concept ID)` decisions.
  The V6 schema stays frozen: confirmations use its existing User tag field.
  Rejections and matching tag removals commit atomically with immediate durability,
  preserving pending edits. Both authored tables belong in normal database backups.
- Disposable `db/embeddings.redb`: `concept_vectors_v1` caches text vectors and
  scoring metadata by exact model ID + vocabulary release. `auto_tags_v1` stores
  predictions, confidence, displayed/hidden placement, thumbnail version, and
  model/vocabulary provenance. `concept_learning_v1` stores derived prototypes and
  classifier coefficients under that same model/vocabulary key. Deleting this file
  does not delete preferences, User tags or Rejections.
- The worker validates vectors and preserves the catalog's calibrated activation
  gate. A vocabulary release rematerializes from existing photo embeddings; only
  its Concept prompts need text inference. User tag aliases and per-photo tag
  changes rematerialize from cached vectors too.
- Reconciliation remembers missing vectors. Each embedding write invalidates only
  its photo, avoiding repeated vector reads for the entire pending backfill.
- Owner image records alone include `autoTags`, `autoTagsEnabled`, and
  `autoTagsPending`. Every prediction contains `concept`, `label`, `confidence`,
  and `displayed`; only displayed predictions become chips. Anonymous and share
  records omit all three fields. They are not in the public metadata allow-list.
- `GET /get/auto-tags/<hash>` requires owner authentication. The metadata row
  checks asynchronously while pending (bounded to two minutes per mount).
  `PUT /put/edit_auto_tags` requires writable owner access and accepts either
  `{hash, enabled}` or `{timestamp, selection, enabled}`. Selection validation uses
  the existing snapshot identity/generation checks and publication lock order.
- `PUT /put/auto_tag_decision` accepts `{hash, concept, action}` or
  `{timestamp, selection, concept, action}`, with `action: "confirm" | "reject"`.
  It returns `{photos, label}` using the canonical French label. The owner-only
  `POST /post/auto_tag_selection` previews `{photos, publicPhotos, concepts}` for
  the same target. Decisions synchronously rematerialize from cached vectors;
  no inference is requested, even when the sidecar is offline. Materialization
  and authored writes share the persistence lock so an old worker snapshot cannot
  undo a completed decision. Owner reads also suppress stale rejected/confirmed
  predictions if derived storage is temporarily unavailable.

Owner tag search/facets combine User and Auto tags with provenance and parent
expansion; see `SEARCH.md`. The Concept review view remains a follow-up slice.
Calibration fix `infra-urocissa-autotag-sjfz.16` replaces the positive-enriched
sampling and observed-precision-only gate. Vocabulary `2026-11-08.2` uses
balanced verified classes plus observed and equal-prior precision gates.
Cloud and Snow fail calibration and remain inactive; independent cat/stork and
neutral-image regressions pass. See `CONCEPT_VOCABULARY.md`. The separate live
owner quality review is still required; this source fix authorizes no rollout.
No tagging failure blocks uploads or the photo embedding queue. Derived read
errors show a pending owner state without breaking gallery reads.

## Learning progress and recalculation (.18)

Settings → Photo indexing and the Concept review toolbar show whether Auto tags
are current, recalculating or awaiting an update, the last successful completion
time and the number of displayed suggestions added/removed. An unchanged result
is explicit. Automatic feedback-driven updates and manual recalculation use the
same status. Completion metadata is disposable and starts unknown after restart;
failed runs keep the previous successful time and result.

Every owner tag facet now opens Concept review, including personal Concepts with
no Auto matches. Review shows current positive/negative example counts and the
actual fitted learning method: built-in visual detection, personal similarity,
learned classification, inactive or pending. Four positive personal examples
explain that one more tagged photo is needed. Five positive and five explicit
negative examples permit classification, without promising new matches or better
accuracy. These are example/processing counts, not measured precision.

`GET /get/auto-tag-status` is owner-only. Writable owners can invoke
`PUT /put/recalculate_auto_tags` to retrain and recalculate eligible indexed
photos using cached vectors, with no image/text inference or re-embedding.
Rejections, User tags, publication settings and Auto-tags-off preferences remain
authored state. Overlapping manual requests return 409; a failed run can be
retried. New feedback/embeddings invalidate the current-status claim, so an older
snapshot cannot mark newer edits as processed. Guests and share visitors cannot
read the status or run recalculation; read-only owners can inspect status only.

This source feature does not change the independent rollout acceptance in `.13`.

## Local acceptance (2026-11-08)

### Personal Concepts and learning (.7)

Every User tag that aliases no built-in Concept defines a personal Concept. Its
stable ID is `personal:` plus the BLAKE3 hash of the trimmed, Unicode-lowercased
name. Case/whitespace variants share a Concept; the display label is an authored
User tag. Album/video tags do not count as photo examples. The existing owner
confirmation/rejection controls and search/facets also accept these Concepts.

At least five distinct tagged photos with current, valid embeddings are required
before a personal Concept can auto-apply. Its fallback is the normalized mean of
those positive embeddings, with a cosine similarity cutoff of 0.8; the confidence
is `sigmoid(12 * (cosine - 0.8))`. A degenerate mean stays inactive until a
classifier can be trained. These are precision-oriented starting defaults, not
calibrated probabilities or proof of production quality.

With at least five positive and five explicit negative examples, either a built-in
or personal Concept uses an L2-regularized, class-balanced logistic classifier over
the embedding and its fallback score. Positive examples are authored User tags
(including confirmations and built-in aliases); negatives are permanent
Rejections, including tag removal. Unlabelled photos never count as negatives.
A conflicting Rejection wins. The learned confidence cutoff is 0.8. Below the
training gate, built-ins keep their existing calibrated scoring/activation rules
and personal Concepts keep their gated prototype. Opting a photo out of Auto tags
does not discard its authored training examples.

Each feedback batch retrains from cached embeddings and rematerializes the whole
library without inference. Completing/changing an example's embedding also
retrains; ordinary unlabelled backfill only invalidates the completed photo.
Examples are sorted by photo ID, and fitting starts from zero with fixed
iterations. Coefficients are disposable: restarts retrain from current authored
state, never reuse potentially stale saved weights. Rebuilding embeddings and
the derived file with the same model/vocabulary and feedback reproduces the same
predictions. Removing a personal User tag commits its stable Rejection immediately,
even when the tagger is offline.

Local optimized verification: 323 backend tests pass (four opt-in tests ignored),
including training gates, duplicate/alias counting, deterministic rebuilds, a
two-cluster Concept missed by its prototype, and real owner HTTP feedback in
public/private gallery modes. A single explicit synthetic benchmark measured
retraining plus full materialization of 7,100 photos, 768 dimensions and 72 built-in
Concepts plus one personal Concept at 282 ms. This is a local measurement, not a
beesslc benchmark. The later owner quality review and rollout gates still apply.
No frontend source changed; the hybrid-scroll gate was not requested or run.

### Original Auto-tag acceptance

Only disposable local data and public/synthetic fixtures were used. With the
pinned CPU sidecar (two threads), a genuinely new default-on PNG had materialized
tags 381 ms after upload/thumbnail completion (inventory verified 8 → 9). An off
upload still reached 7/7 embedded photos. With the sidecar container stopped,
re-enabling that photo returned ready predictions at the first read (1 ms),
proving no new image/text inference was needed. These are focused smoke checks,
not production-host benchmarks or throughput promises.

Automated coverage includes cached Concept reuse, vocabulary invalidation,
per-photo and library aliases, authored preference survival after derived-file
loss, HTTP owner/share/anonymous boundaries, default/off/duplicate upload choices,
selection controls, queue snapshots, and separate outlined chips. Production
rollout and the expensive hybrid-scroll gate remain opt-in.

Frontend checks: 318 tests, type check, lint and production build pass. Backend:
314 tests pass, 3 intentionally ignored; fmt and clippy pass with existing
unrelated warnings only. Vuetify 4.1.5 APIs guided the implementation: no custom
CSS or forced themes. Rendered upload dialog, chips/tooltips and photo menu were
checked in desktop light/dark, narrow 390px desktop, and 390px touch/mobile
light/dark. Dialog/menu surfaces switch colors with the live global theme;
tooltips inherit its theme. Auto-tag survives a page reload while publication
resets. Anonymous photo UI has no Auto tags or owner controls.

The previous deployed-reader candidate `3.4.2-audience-f95f452` also booted against
a disposable copy written by this slice: it opened the V6 typed records table,
read all 7 then-existing media records, and launched Rocket successfully. The
new authored preference table is additive and old readers ignore it.

## Authored feedback acceptance (.6)

The integrated `.6`/`.9` branch passes 319 backend tests (three intentionally
ignored), 332 frontend tests, fmt, clippy (baseline warnings only), TypeScript,
ESLint and the production build. No dependency or custom CSS was added.

HTTP coverage verifies owner-only single/bulk decisions, read-only refusal,
snapshot validation, ordinary French User tags visible to visitors, permanent
removal feedback, and stale derived search/facet suppression while preparation
is unavailable. Rejections survive restart, vocabulary/model changes and derived
file deletion. Slow text preparation does not block decision rematerialization.

A fresh offline build of pre-feature `production/reconciled` at `f95f452f` boots
against the authored database, serves the confirmed `chat` User tag anonymously,
omits Auto fields, and leaves the Rejection table intact. Reproduce that extra
binary test by supplying the old executable explicitly:

```sh
UROCISSA_PREVIOUS_BINARY=/absolute/path/to/previous/urocissa \
  cargo test --offline --locked --profile dev-release previous_binary_serves
```

With local synthetic 768-dimensional cached embeddings, each 500-photo HTTP
confirmation/rejection plus synchronous rematerialization completed in 23–26 ms
in `dev-release`, and under 1.34 s in the unoptimized debug build. Both public-
gallery modes were exercised, with no image or text inference. These measurements
are local acceptance checks, not host benchmarks.

Focused component/browser checks covered both chip actions, bulk count and
publication text, keyboard selection, light/dark and live overlay theme changes,
390px width without horizontal overflow, and loading/read-only/error states.
Temporary preview services were stopped. The hybrid-scroll gate was not requested
and was not run. No production backup, deployment, backfill or host benchmark
was performed.

## Concept review acceptance (.8)

Owners can open **Review across the library** from an Auto chip or a tag facet
with Auto matches. The existing gallery displays that Concept's current Auto-tagged
photos, including Trash, least confident first (photo ID breaks confidence ties).
The view excludes disabled photos, stale model/vocabulary/thumbnail predictions,
deleted photos, videos, confirmations and permanent Rejections. It reads cached
predictions only: no new storage, image/text inference or learning behavior.

`POST /get/prefetch?review_concept=<Concept ID or label>` accepts a null body and
returns the ordinary gallery snapshot plus `conceptReview`: canonical `concept`,
`label`, `positiveExamples` and `negativeExamples`. Other filters, semantic
selections and non-default sorts are refused. Anonymous/share requests are denied
before Concept lookup. Example counts use current valid embeddings, count each
photo once, include authored examples on opted-out photos, and let Rejections win
over conflicting User tags. The toolbar shows progress toward the five-positive /
five-negative classifier gate; reaching the count is not a quality/rollout approval.

Bulk confirm/reject uses the existing feedback endpoints and compact selection
descriptors. The confirmation preview pins the Concept, selection and snapshot;
a changed snapshot requires a new selection. Successful feedback updates owner
photo-chip caches and refreshes the review/facets without a page reload. Read-only
mode permits inspection but not decisions. Errors retain the selection for retry.

Verification: 324 backend tests pass (four opt-in tests ignored), including real
owner/anonymous/share HTTP tests in public/private gallery modes, confidence
ordering, current-embedding counts, personal Concepts and offline stale-row
suppression after feedback. All 343 frontend tests, type check, lint and production
build pass; fmt and clippy pass with existing unrelated warnings only.

Focused Playwright checks used the real frontend/gallery with a synthetic local
HTTP fixture: both bulk actions refreshed to empty without reload, failed writes
retried safely, and loading/listing-error/empty/read-only states worked. Desktop
light/dark, live overlay theme changes, 390px desktop and touch-mobile layouts,
and long Concept labels were checked without horizontal overflow. This also found
and fixed `infra-ap2l`: gallery tile buttons now support Enter/Space, including
Shift range selection and the desktop hover selector. A tab-order regression
failed before the fix and passes after it.

Vuetify 4.1.5 components/utilities supply the new view and theme-aware dialog; no
custom CSS or dependency was added. Temporary local services were stopped. The
hybrid-scroll gate was not explicitly requested and was not run. No production
backup, deployment, backfill or host benchmark was performed.
