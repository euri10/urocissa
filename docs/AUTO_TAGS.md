# Owner-only Auto tags

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
  model/vocabulary provenance. Deleting this file does not delete preferences.
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
expansion; see `SEARCH.md`. Learned Concepts and review UI remain follow-up slices.
Calibration fix `infra-urocissa-autotag-sjfz.16` replaces the positive-enriched
sampling and observed-precision-only gate. Vocabulary `2026-11-08.2` uses
balanced verified classes plus observed and equal-prior precision gates.
Cloud and Snow fail calibration and remain inactive; independent cat/stork and
neutral-image regressions pass. See `CONCEPT_VOCABULARY.md`. The separate live
owner quality review is still required; this source fix authorizes no rollout.
No tagging failure blocks uploads or the photo embedding queue. Derived read
errors show a pending owner state without breaking gallery reads.

## Local acceptance (2026-11-08)

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
