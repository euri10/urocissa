# Background photo embeddings

This is the indexing foundation for the auto-tagging epic. It stores photo vectors
and supports [owner semantic search](SEARCH.md#10-semantic-search-owner-only).
Auto tags and similar-photo UI are separate slices.

## Enable

Run the [stateless sidecar](../gallery-tagger/README.md) on a private network.
Set `private.taggerUrl` in the gallery's `config.json`, for example
`"http://tagger:5674"`, and restart the gallery. `UROCISSA_TAGGER_URL` overrides
that setting; an empty override disables it. With neither configured there is no
worker, cache, or indexing UI. Keep the sidecar off the public network.

The owner sees **Photo indexing** in Settings: coverage, pending/retrying photos,
connection status, offline-since time, and the device reported by the sidecar.
Pause stops selecting old photos; an in-flight request finishes and new uploads
continue. Pause is process-local: restarting resumes backfill. Read-only mode
disables the pause control. Changing the URL requires a restart.

## Processing and failure isolation

- Only images participate. Videos and albums are excluded.
- The worker reads the existing viewer JPEG, after thumbnail publication. Uploads
  only send a coalesced, non-blocking wake signal; they never call the tagger.
- The durable queue is the photo library minus current cached embeddings. A
  restart reconstructs it, preserving completed work. Every five seconds the
  worker also reconciles changes in case a wake signal was coalesced.
- New/changed photos take priority over backfill. There is one inference request
  at a time and a 100 ms gap between backfill requests.
- Offline probes and individual failures retry with exponential backoff (up to
  256 seconds). HTTP requests time out after 10 seconds (connect: 3 seconds),
  reject redirects, and bound response sizes. One broken thumbnail does not
  prevent other photos being indexed. Gallery health/authentication is unchanged.
- A new model ID schedules all photos again. Model identity and vector dimensions,
  finite values and unit norm are validated before storage. IDs must identify an
  immutable model. The sidecar must not reuse an ID for different weights.
- Rotation/reindex changes the thumbnail version; stale in-flight results are
  discarded. Deleted photos are excluded from the current inventory. Cached old
  models/deleted-photo vectors are not automatically compacted in this slice;
  semantic search intersects vectors with the live authorized photo inventory and
  requires a matching thumbnail version. Old model/frame vectors are excluded.

## Storage and rollback

`<data-root>/db/embeddings.redb` is a separate, disposable redb file. Rows are keyed
by `(item ID, model ID, frame index)`; this pass uses frame zero only. Values contain
the thumbnail version and vector. Metadata remembers the last model/device so
coverage remains meaningful across an offline restart. The cache uses a 16 MiB
redb page-cache budget. No V6 records or authored tag/audience data are changed.

For recovery, stop the gallery, move **only** `db/embeddings.redb` aside, and restart.
The gallery re-embeds its photos. Never remove `index_v6.redb`, audience data, or
the `db` directory. Moving the derived file aside retains a recoverable copy.

Rollback to the previously pinned gallery binary/configuration needs no database
conversion: it ignores `embeddings.redb`. Disable/remove the sidecar configuration
and retain the cache for a later upgrade. Actual production deployment, backup
verification and host performance/quality gates remain separate epic tasks.

## API and checks

- `GET /get/embedding-status`: owner-only JSON; 404 if disabled.
- `PUT /put/embedding-backfill` with `{"paused":true}`: owner-only, writable mode;
  returns the updated status. Anonymous, public and share principals get 401;
  read-only mutations get 405.
- `/get/config` exposes `embeddingEnabled` only as true for an owner with the
  service configured. The internal URL is not part of the public config.

From `gallery-backend`, run `cargo test --locked --bin urocissa embedding` for
fake-sidecar outages, slow inference, retry isolation, upload priority, pause,
restart, model/thumbnail invalidation, stale-result prevention, derived-cache
rebuild, unchanged populated V6 records, HTTP contract and access-control checks.
Run the full backend suite after building the frontend (its route tests require
`gallery-frontend/dist/assets`). Frontend validation is `npm run check`,
`npm run test:unit`, `npm run lint`, then `npm run build:only`, sequentially.

### Local acceptance evidence (2026-11-07)

A disposable, password-protected gallery accepted two JPEG uploads with the
tagger stopped (470 ms and 436 ms). Its real status endpoint reported two pending
photos and an offline timestamp. Starting the locally built SigLIP 2 sidecar on
CPU drained both. Restarting the gallery with the sidecar stopped retained both
completed embeddings; moving the derived cache aside reconstructed the pending
set and recovered both through real inference. The pre-embedding binary from
`1868dd5b` then opened the same two-record V6 database, ignored the cache, and
passed the existing empty-password/401 health probe. All temporary services were
stopped afterwards. This is local correctness evidence, not an N100 benchmark.

The status component was checked at 1280px and 390px in both themes, including
pause/resume, offline, empty, read-only, HTTP failure and loading states. No
horizontal overflow or clipped labels remained. The opt-in hybrid-scroll gate
was not run.
