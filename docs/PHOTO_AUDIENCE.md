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

Counts/facets/tags belong to `.7`, album audience and
private-cover handling to `.9`, and further share-link changes to `.10`.

## Photo publishing controls (`.8`)

The upload page's Add Files / Choose Files actions open a theme-inheriting
chooser with **Publish immediately** off. The switch resets on every open and
is never persisted. The choice is captured when the native picker opens and
copied into each queued item, including retries. Drops and clipboard uploads
remain private. `POST /upload?publish_immediately=true` explicitly publishes
after ingestion; omitted/false leaves new records private. Shared uploads
cannot publish, even with a forged query parameter. Reuploading an existing
hash without publication preserves that record's existing audience.

Owner photo menus expose Publish or Make private; photo selection menus expose
both, including inside an album. These actions change photo/video audience only;
album cards are rejected atomically if included in a batch. Private and missing
audiences have an owner-only lock badge in the grid, hidden in shared views.

`PUT /put/edit_photo_audience` requires owner authentication and writable mode.
The JSON body is either `{ "hash": "<photo hash>", "audience": "public" }`
or `{ "timestamp": 123, "selection": { "mode": "explicit", "indices": [0] },
"audience": "private" }`. Existing `allExcept` selections are supported.
Hash and snapshot selection forms cannot be mixed. The storage setter validates
all media targets and commits the batch plus one audience revision atomically.
Anonymous and share-only credentials cannot change audience.

Owner data envelopes add optional `audience` alongside `abstractData`, `timestamp`
and `token`; direct resource responses use the same envelope under `data`.
Anonymous/share responses and album records omit it. Frontend worker and direct
resource parsers carry the envelope value into the client media model without
changing the frozen V6 record. After accepted mutations, the frontend updates
cached media, clears selection/tokens, and refreshes both collection hosts and
loaded direct resources. Existing server revision checks immediately invalidate
old snapshots and enforce visibility on future metadata and media requests.

Fixture regressions cover upload default/true/false and switch reset, owner-only
controls/badges, both response parsing paths, single/batch/allExcept mutations,
anonymous/share/read-only refusal, atomic invalid batches and stale snapshots.

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
existing files still require `.5`.

This batch is source integration, not a complete private-gallery rollout.
Do not repin or deploy it as completion of the epic while `.5`, `.7`, `.9`,
`.10` and the production gates in `.11` remain outstanding.

## Interfaces for remaining tasks

`.7` can use `PhotoVisibility::allows_cached` for tree-backed aggregates and
`allows_data` for logical records. Photo controls use
`DataStore::photo_audiences().get(hash)` and `set_photo_audiences`; the setter
invalidates previous snapshots through its durable revision. `.9` must replace
the adapter's documented album compatibility behavior and add album migration
without rerunning the photo migration. Cache expectations in thumbnail mutation
tests require `no-store` for both tiers. Preserve owner-only selection entry
through `collectionStore.enterEdit()` when adding publishing controls.
