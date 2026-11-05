# Photo audience storage and read policy

`audience` is a separate table in `db/index_v6.redb`, keyed by photo/video
hash. `0` means private and `1` means public. Missing entries and unknown values
mean private. The frozen V6 `records` table, bitcode types and payloads are
unchanged. Albums retain their existing behavior until the album audience task.

Before ingestion or HTTP startup, storage preparation runs the one-time
`existing_photos_public_v1` migration. It inserts public entries for existing
photos/videos (not albums), preserving explicit audience entries. Entries and
the completion marker in `audience_meta` commit together with immediate
durability. A decoding/storage error aborts startup and the entire migration;
retrying cannot leave partially published data. After the marker exists,
restarts never publish missing entries. A fresh database is marked while empty,
before any upload, folder import or sync can run.

Normal record writers do not publish photos. Upload, folder import, sync and
pending write-behind records therefore default to private. Removing a record
removes its grant in the same transaction; new records also discard stale grants
left by an older binary, so reingestion of a deleted hash stays private.
Explicit publication uses `DataStore::set_photo_audience`. Audience changes
increment a durable revision atomically with the audience write.

`Viewer::can_view` is the pure policy: owners see public/private/missing entries,
anonymous visitors see only public non-trashed photos, and a validated share
visitor sees non-trashed members of that specific share's album regardless of
photo audience. `PhotoVisibility` adapts this policy for durable/logical records
and cached tree records. Use it for additional read paths rather than adding
handler-specific audience rules.

Prefetch filters before building snapshots in every sort order. Data, rows,
direct resources and all three media routes recheck visibility. Timestamp
tokens and query caches carry the viewer and audience revision: an owner
snapshot cannot be replayed anonymously, and an audience change requires a new
snapshot. Old timestamp tokens without these fields are rejected; clients must
refresh. Share permissions are resolved again on each timestamp request.
Media responses use `Cache-Control: no-store` (originals also specify `private`),
so future requests cannot bypass visibility checks through a shared or browser
cache. Data already downloaded
before a privacy change cannot be recalled.

## Fail-closed rollback

Production migration/deployment, backup verification, image pinning and rollback
execution belong to `infra-urocissa-private-audience-itz1.11`. This change does
not authorize those operations.

Before the first production migration, take and verify the backup required by
the deployment contract. The previous binary can read the same V6 records and
ignores the extra redb tables, but it does not enforce photo audience. A code
rollback **must** redeploy the previously pinned image with
`UROCISSA_PUBLIC_GALLERY=0` and the owner password/authentication configuration
retained. Never run that image with the public gallery enabled after private
photos have been ingested. Preserve `urocissa_data` and its volumes; do not
delete the audience tables or completion marker, reverse the migration, or
restore an older database as part of an ordinary code rollback.

Invalidate outstanding bearer tokens during that rollback by rotating the JWT
signing key through the operator's secret configuration, and require a fresh
owner login. The previous binary's timestamp routes trust bearer tokens without
checking the gallery flag or the new viewer/revision claims. Disabling the
gallery alone stops new anonymous sessions but does not revoke already issued
tokens. This invalidation is required for an immediate fail-closed rollback;
it is an operator step for `.11`, not an operation performed by these tests.

Do not warm or reuse an HTTP cache of old public media during cutover/rollback.
Previously downloaded public files remain outside the server's control.

## Local verification

Migration tests create disposable databases containing synthetic frozen V6
records. They check byte preservation, compatibility with the pre-audience
typed reader, atomic failure/retry, one-time migration across reopen, and
private defaults for later record writes. HTTP tests run Rocket's actual read
routes in isolated child processes with dummy credentials, synthetic media and
temporary databases; they open no listener and contact no external service.

For backend-only validation without a frontend build, first create the empty,
ignored `gallery-frontend/dist/assets` directory required by the existing Rocket
configuration test. Then, from `gallery-backend`:

```sh
cargo test --offline --locked storage::audience
cargo test --offline --locked public::structure::audience
cargo test --offline --locked router::audience_http_tests
cargo test --offline --locked
cargo fmt --all -- --check
cargo clippy --offline --locked --all-targets
```

Counts/facets/tags belong to `.7`, publish controls to `.8`, album audience and
private-cover handling to `.9`, and further share-link changes to `.10`.

## Integrated first batch

Tasks `.1`, `.2`, `.3`, `.4` and `.6` were merged from the same `55acb30`
production-source base, preserving their commits and branches. Reconciliation
is tracked by `infra-urocissa-private-audience-itz1.12`; `.13` fixes share
authorization forwarding through the frontend data worker.

Visibility checks run before public metadata redaction and original-download
restrictions. Public originals return 403; private objects remain undiscoverable
with 404. Owners retain full metadata and original access. Share records receive
the public allow-list, including private photos explicitly granted by the share.
The resource helper takes `&PhotoVisibility`; its response policy derives owner
status from that validated viewer.

Prefetch cache keys include both hidden-field search policy and viewer/audience
revision. Anonymous Path expressions are rejected before cache lookup; free text
cannot match paths, even after the owner has cached the same query. Scoped
timestamp tokens require the matching request identity. The main-thread Axios
interceptor and the data worker both forward the active share context; worker
messages carry a fresh header object for each data/row request so switching or
leaving a share does not retain its credentials.

Combined HTTP regressions cover owner-cache reuse, private-photo exclusion,
metadata redaction for shared private photos, media/original authorization and
stale snapshot rejection. Frontend tests cross the real producer/worker message
boundary and render the combined read-only metadata panel with the date hidden.
The route-enumeration gate retains the exact existing public POST prefetch read
exception. New video compression strips source container and stream metadata;
existing files require the owner maintenance operation below. Its production
execution remains part of `.11`.

This batch is source integration, not a complete private-gallery rollout.
Do not repin or deploy it as completion of the epic while `.7`, `.8`, `.9`,
`.10` and the production gates in `.11` remain outstanding.

## Existing compressed-video metadata maintenance (`.5`)

An authenticated owner can start the background operation with
`POST /post/video/strip-metadata` (no body). It returns HTTP 202 with `jobId`;
another start while it runs returns 409. Read-only mode refuses a start with
405. `GET /get/video/strip-metadata` returns the latest job, or `null` before
the first run. Both endpoints require owner authentication; public gallery,
share and expired tokens cannot authorize them. No automatic startup job runs.

The status contains `jobId`, `state`, `report` and `error`. States are `running`,
`completed`, `completedWithErrors` and `failed`. After a complete scan, `report`
contains `total`, `changed`, `unchanged`, `failed`, up to 20 error summaries and
`verification`. Each initial target contributes to exactly one outcome count.
Verification is a fresh scan with `total`, `checked`, `locationTagged`, `failed`
and `passed`; unreadable files or traversal errors prevent `passed`. Completion
requires both zero operation failures and passing verification. Status is
process-local and retains only the latest run; save the returned report as
delivery evidence before restarting the service or starting another run.

The scan covers compressed MP4 filenames accepted by the media route, including
orphaned, nested and versioned derivatives under `object/compressed`. It does
not follow directory symlinks, and file symlinks fail verification. Imported
originals, thumbnails, database records and audiences remain untouched. Each
file is protected by the existing per-object media mutation lock. Source tags
are stripped using the same container/stream policy as new compression, with
chapters also removed. FFmpeg uses `-c copy`, preserving compressed streams;
there is no re-encoding. Already-clean files are not rewritten.

Each replacement is a unique hidden sibling file. Before publication the job
checks the metadata allow-list, packet counts, stream properties/timing,
rotation, codec extradata and compressed stream hashes. It syncs the file and
uses one rename over the destination, with no missing-file interval. A command,
verification or rename failure preserves the previous file and counts as failed.
Commands time out after five minutes and are killed on cancellation. A canceled
task removes its temporary file; a killed process may leave an unserved hidden
temporary file, which subsequent runs ignore. Rerunning is safe. These are
process-interruption guarantees, not a substitute for storage-level durability
or a verified backup.

Verification is an observation of the fresh scan, not a global storage freeze.
Quiesce external writers for the final delivery audit. New application-generated
videos already use the shared sanitization policy. Production execution still
requires the fresh verified `urocissa_data` backup and explicit delivery steps
in `infra-urocissa-private-audience-itz1.11`; this implementation was exercised
only against generated fixtures.

Targeted fixture checks (using the isolated target directory for the worktree):

```sh
cargo test --offline --locked operations::video_metadata
cargo test --offline --locked strip_video_metadata
cargo test --offline --locked compressed_videos_strip_source_metadata
cargo test --offline --locked router::anonymous_route_tests
```

## Interfaces for remaining tasks

`.7` can use `PhotoVisibility::allows_cached` for tree-backed aggregates and
`allows_data` for logical records. `.8` can use
`DataStore::photo_audiences().get(hash)` and `set_photo_audience`; the setter
invalidates previous snapshots through its durable revision. `.9` must replace
the adapter's documented album compatibility behavior and add album migration
without rerunning the photo migration. Cache expectations in thumbnail mutation
tests require `no-store` for both tiers. Preserve owner-only selection entry
through `collectionStore.enterEdit()` when adding publishing controls.
