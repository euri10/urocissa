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
Media responses use `Cache-Control: no-store`, so future requests cannot bypass
visibility checks through a shared or browser cache. Data already downloaded
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

## Integration with sibling tasks

`.2` overlaps `router/get/get_data.rs`, `get_resource.rs`, `get_img.rs` and
possibly timestamp guards. Retain both the shared visibility check and `.2`'s
response redaction/original-download restrictions. Use
`PhotoVisibility.viewer == Viewer::Owner` where `.2` needs the owner distinction;
the resource helper now takes `&PhotoVisibility` instead of `is_admin`. Check
photo visibility before reporting the separate original-download restriction.

`.3` overlaps `router/get/get_prefetch.rs`. Preserve its hidden-field search
rules alongside visibility filtering, viewer/revision cache keys and scoped
timestamp issuance. `ClaimsTimestamp::new` now requires the resolved share,
timestamp, viewer and audience revision. Old unscoped tokens are intentionally
invalid. Test-module registration in `router/mod.rs` may also need combining.

`.7` can use `PhotoVisibility::allows_cached` for tree-backed aggregates and
`allows_data` for logical records. `.8` can use
`DataStore::photo_audiences().get(hash)` and `set_photo_audience`; the setter
invalidates previous snapshots through its durable revision. `.9` must replace
the adapter's documented album compatibility behavior and add album migration
without rerunning the photo migration. `.1` and `.4` have no expected direct
implementation-file overlap. Cache expectations in thumbnail mutation tests
now require `no-store` for both tiers.
