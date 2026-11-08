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

## Storage and response boundaries

- Main database: `auto_tags_off_v1` stores authored, default-on preferences. It is
  additive; frozen V6 records and their User tags are unchanged.
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

Confirm/reject feedback, learned Concepts, review UI, and Auto tag search are
separate follow-up slices; predictions are not yet included in User tag search.
Known quality blocker `infra-urocissa-autotag-sjfz.16`: fresh exact-model inference
also fires Cloud and Snow on the public cat fixture. Thresholds were not adjusted
by eye in this slice; recalibration and negative-image regressions must pass
before live rollout (`.13` explicitly depends on this fix).
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
