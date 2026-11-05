# Photo and album audience storage and read policy

`audience` is a separate table in `db/index_v6.redb`, keyed by photo/video
hash or album ID. `0` means private and `1` means public. Missing entries and unknown values
mean private. The frozen V6 `records` table, bitcode types and payloads are
unchanged. Photo and album audience are independent.

Before ingestion or HTTP startup, storage preparation runs the one-time
`existing_photos_public_v1` migration. It inserts public entries for existing
photos/videos (not albums), preserving explicit audience entries. Entries and
the completion marker in `audience_meta` commit together with immediate
durability. A decoding/storage error aborts startup and the entire migration;
retrying cannot leave partially published data. After the marker exists,
restarts never publish missing entries. A fresh database is marked while empty,
before any upload, folder import or sync can run.

The independent `existing_albums_public_v1` migration grants public audience to
existing albums only, preserving explicit entries. Its entries, revision and
completion marker commit atomically with immediate durability. It does not run
the photo migration or alter photo audience. Startup checks each migration's own
marker; an already completed photo migration remains completed. Fresh databases
receive both markers before ingestion or album creation, so new albums are private.

Normal record writers do not publish photos or albums. Upload, folder import, sync and
pending write-behind records therefore default to private. Removing a record
removes its grant in the same transaction; new records also discard stale grants
left by an older binary, so reingestion of a deleted hash stays private.
Explicit publication uses `DataStore::set_photo_audience`. Audience changes
increment a durable revision atomically with the audience write.

`Viewer::can_view` is the pure policy: owners see public/private/missing entries,
anonymous visitors see only public non-trashed photos and albums, and a validated share
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

## Album publication and presentation

The owner-only `PUT /put/album_audience` accepts `albumId`, `audience` (`private`
or `public`) and an optional `publishPhotos` boolean, defaulting to false.
Publishing an album alone never changes photo audience. Making an album private
also leaves its photos unchanged. Only the explicit "Publish album and all its
photos" action sets `publishPhotos: true`; it requires public audience and
publishes all current non-trashed image/video members in the same transaction
as the album. Adding photos later preserves their own audience; private photos
remain private unless explicitly published.

The route waits up to 30 seconds for pending writes, then holds the persistence
lock and tree read lock while confirming no new pending edits and committing.
`DataStore::set_album_audience` validates the durable album and uses durable
membership for the combined action. A failure rolls back every audience change;
a successful action advances the shared durable audience revision once.

Anonymous album lists, direct resources and prefetch results exclude private
albums. Album membership filters cannot expose a hidden album's members, and
anonymous photo responses omit hidden album IDs. Public album counts and sizes
include only visible public photos. The selected cover is used only if it is a
visible current member; otherwise the newest public member supplies the cover,
thumbnail hash and cache version, or all cover fields are empty for a placeholder.
This response projection never rewrites the owner's chosen cover or stored counts.
Share access remains governed by the integrated share policy; share-link changes
belong to `.10`.

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
cargo test --offline --locked router::album_audience_http_tests
cargo test --offline --locked
cargo fmt --all -- --check
cargo clippy --offline --locked --all-targets
```

Album fixtures cover private list/direct/search exclusion, visible member counts
and sizes, cover fallback and empty placeholders, unchanged owner covers,
owner-only publication, pending membership edits, later private additions, and
snapshot invalidation. Frontend tests cover all three album actions, writable
owner gating, rejected requests, schema defaults and snapshot refresh.

Generic counts/facets/tags belong to `.7`, photo publish controls to `.8`, album
audience and private-cover handling to `.9`, and further share-link changes to `.10`.

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
Do not repin or deploy it as completion of the epic while `.5`, `.7`, `.8`,
`.10` and the production gates in `.11` remain outstanding.

## Interfaces for remaining tasks

`.7` can use `PhotoVisibility::allows_cached` for tree-backed aggregates and
`allows_data` for logical records; both now apply audience to albums as well as
photos. `.9` changes only the album-list branch of `router/get/get_list.rs`;
reconcile that branch with `.7`'s generic facets changes. The shared
`operations::album_visibility::visible_data_to_database_timestamp_return` helper
owns anonymous album counts/covers and hidden album membership redaction for
both data and direct-resource responses. Keep those projections when adding
other response fields.

`.8` can use
`DataStore::photo_audiences().get(hash)` and `set_photo_audience`; the setter
invalidates previous snapshots through its durable revision. The existing
`photo_audiences()` accessor also reads album IDs, and `Audience` serializes as
lowercase `private`/`public`. `.9` adds `set_album_audience(hash, audience,
publish_photos)` without changing the photo setter or frozen V6 schema. Preserve
the independent album migration marker when reconciling storage changes; never
clear or rerun the completed photo migration. Cache expectations in thumbnail mutation
tests require `no-store` for both tiers. Preserve owner-only selection entry
through `collectionStore.enterEdit()` when adding publishing controls.
