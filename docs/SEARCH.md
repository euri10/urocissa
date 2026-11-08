# Examples of Search Queries

Here are some examples showcasing how you can use the search syntax in Urocissa:

### 1. Search by Any

```
any: "samsung"
```

Search for data that matches the **samsung** in any field.

### 2. Simple Extension Search

```
ext: "jpeg"
```

Search for data extension that is **jpeg**.

### 3. Search by Model and Type

```
or(model: "SLT-A57", type: "image", model: "SLT-A58")
```

Search for data related to either camera model SLT-A57 or SLT-A58, or image type.

### 4. Search by Tag

```
tag: "nature"
```

Search for data that is tagged with **nature**.

### 5. Search by Make

```
make: "nikon"
```

Search for data related to the **Nikon** make.

### 6. Not Expression Search

```
not(model: "outdated")
```

Search for data that does **not** have the **outdated** model.

### 7. Search by Status (Favorite, Archived, Trashed)

With the new flags update, you can explicitly search for item statuses using boolean values (`true` or `false`).

```
favorite: true
```

Search for data marked as **favorite**.

```
trashed: false
```

Search for data that is **not** in the trash.

```
archived: true
```

Search for data that is **archived**.

### 8. Complex Combination

```
and(type: "image", trashed: false, or(any: "sony", any: "samsung"))
```

Search for **image type** data that is **not in the trash**, and includes items that have **sony** or **samsung** in any field.

### 9. Search by Visibility (Owner Only)

As an owner, open **Advanced search → Visibility → Private**, or enter:

```
audience:"private"
```

Use `audience:"public"` for published items. Unquoted `audience:private` and
`audience:public` also work. Combine visibility with other criteria, for example:

```
and(audience:"private", type:"image", tag:"family")
```

The filter applies to photos, videos, and each album's independent visibility.
It stays within the current gallery or album. Visitors and share-link viewers
cannot use audience filters. **Clear All** resets Visibility to **All**.

### 10. Semantic Search (Owner Only)

With [photo indexing enabled](PHOTO_EMBEDDINGS.md#enable), open **Advanced search →
Describe a photo** and describe what you want in French or English. Or type:

```
semantic:"enfant à vélo"
```

The search defaults to **Relevance**. The sort button cycles through relevance,
newest first and oldest first while keeping the same selected photos. The
coverage notice shows how many matching photos were indexed or pending when
the search ran; videos are excluded. Run the search again to include newly
indexed photos. Semantic queries stay in local search history; saving them to
private configuration is disabled so the preceding gallery binary can still
read that configuration during rollback.

Combine one description with ordinary filters using `and(...)`, for example:

```
and(semantic:"storks near water", tag:"holiday", audience:"private")
```

Existing gallery/album constraints still apply. Semantic expressions inside
`or(...)` or `not(...)`, multiple descriptions, blank text and descriptions
longer than 2,000 characters return an error. Other filters may still contain
Boolean expressions. Both anonymous visitors and share principals are refused
before inference, including an owner browsing through a share link.

Ranking uses cosine similarity for the sidecar's current model, keeps positive
scores within 15% of the best match, and caps the set at 200 photos. These are
retrieval scores, not calibrated probabilities. The existing compact snapshot
path serves rows, selection and downloads; it does not recompute rankings while
scrolling. Date order gets an ordinary date scrollbar; relevance order has none.

Recent query vectors are cached for one minute (up to 32 queries), with a live
model/health probe even on a hit. If the tagger is offline, a new semantic search
returns 503 with a clear message; ordinary searches and uploads remain available.
An already selected set can still be sorted without contacting the tagger.

The API remains `POST /get/prefetch`, with `{"Semantic":"a sunset"}` as JSON and
`sort=relevance`, `descending`, `ascending` or `random`. Owner semantic responses
add `semanticSearch` with `selection`, `indexedPhotos` and `pendingPhotos`. Send
that `selection` back as the `semantic_selection` query parameter to re-sort the
same set with the exact same filter. Selections are process-local, limited to 64,
and expire after five idle minutes or a restart; expired or mismatched IDs return
409 and require a fresh search. Deleted, changed-thumbnail or no-longer-matching
photos are withdrawn when re-sorting. Non-semantic responses keep their existing
JSON shape. No authored database tables or V6 records change in this slice.

Development checks (from `gallery-backend`):

```sh
cargo test --profile dev-release --locked --bin urocissa semantic -- --nocapture
```

The HTTP gate checks owner/private-photo search, anonymous/share refusal,
filter composition, stale thumbnail exclusion, frozen membership during backfill,
offline behavior and a complete request over 10,000 stored 768-dimensional
vectors. Its inference is a deterministic fixture; the optimized request must
finish within 200 ms. Debug builds have a relaxed one-second budget.

Local acceptance on 2026-11-07: optimized fixture requests took 44–62 ms;
real SigLIP 2 CPU inference over five attributed test photos took 52–60 ms per
request, with French/English cat and stork queries returning the expected photos.
The preceding binary reopened the populated fixture database and retained owner
authentication after search. Desktop, 390px desktop and physical-mobile contexts
were checked in both themes, including live overlay theme changes, loading,
empty results and offline recovery. The opt-in hybrid-scroll gate was not
requested. These are local checks; production rollout and beesslc performance
acceptance remain separate work.

### 11. Similar Photos (Owner Only)

With photo indexing enabled, open a photo's **Media actions → Similar photos**.
The ordinary **All** gallery opens in Relevance order, excluding the source
photo. Select results and use **Batch actions → Batch Edit Tags** to tag them together.
The date/relevance sort cycle preserves the selected result set, just as in text
search. Videos and share-link galleries do not offer this action.

You can also enter `similar:"<64-character photo ID>"`, optionally combined with
ordinary filters using `and(...)`. The API uses `{"Similar":"<photo ID>"}` with
the same `/get/prefetch` endpoint, `sort` and `semantic_selection` parameters,
snapshot selection actions, ranking cutoff, result cap and idle expiry as text
search. Use exactly one `semantic` or `similar` expression, not both, and do not
put it under `or(...)` or `not(...)`. Visitor/share requests are refused with 403
before consulting the embedding index, regardless of sort order.

Similarity uses the source's current thumbnail embedding and the model recorded
in the local index; it does not contact the tagger and works during a sidecar
outage. An unprocessed, stale or invalid source vector returns 409 with **not
processed yet** and a retry hint. A missing/non-photo source returns 404. Indexing
must be enabled and initialized; otherwise the search returns 503. Candidates
without current, valid embeddings are omitted and counted as pending. The
source is excluded before ranking and does not count toward coverage or the cap.
Saving ranked queries to private configuration remains disabled for rollback
compatibility; local search history still works.

The semantic HTTP gate also checks source exclusion, image-neighbour ordering,
stale/missing source vectors, non-owner refusal, ordinary filter composition,
frozen date re-sorts during backfill, existing bulk-selection resolution, and a
complete 10,000-photo / 768-dimensional similarity request (optimized budget:
200 ms). Production rollout remains a separate, owner-approved task.

Local acceptance on 2026-11-08: optimized similarity requests over 10,000 stored
vectors took 50–53 ms. Real-image fixture requests took 0.7–1.5 ms and returned
the other stork image first. Desktop and 390px desktop/touch-mobile contexts
passed both themes, live menu theme changes and the relevance/date cycle.
Selecting all three cat-neighbour results and bulk tagging updated exactly those
photos, not the source. Stopping the local sidecar left fresh image similarity
available while new text searches returned 503; a newly uploaded photo returned
the not-processed 409 message. The full optimized backend suite passed with
`--test-threads=1`; concurrent CPU-heavy checks can exceed the wall-clock timing
budget, so run the isolated HTTP gate above for performance acceptance.

### 12. User and Auto Tags (Owner Only)

Owner `tag:"cigogne"` and `tag:"stork"` searches match the union of User tags
and current Auto tags. French/English Concept names and synonyms refer to the same
Concept, ignoring surrounding whitespace and case. Parent queries such as
`tag:"oiseau"`, `tag:"bird"` and `tag:"animal"` include descendants, including
photos whose User tag names a descendant. Custom User tags keep their exact spelling.
Hidden Auto tags (over the chip cap, suppressed ancestors and implied parents) all
participate in search and facets. Videos have only User tags.

Narrow a tag query by its provenance:

```
and(tag:"cigogne", source:user)
and(tag:"bird", source:"auto", trashed:false)
or(and(tag:"cigogne", source:user), and(tag:"cat", source:auto))
```

`source:user` and `source:auto` also work alone to find items with any tag from
that source. A qualifier in `and(...)` narrows the tag operands in that scope,
including nested Boolean expressions; it does not change sibling `or(...)`
branches. Conflicting qualifiers in one AND scope match nothing. A tag-existence
query such as `and(tag:false, source:auto)` finds items without Auto tags.
Tag/source filters can also be combined with semantic and Similar photos searches.

The Tags page shows one row per Concept, using the library's preferred User-tag
alias or the French label, with counts such as `8 yours + 23 auto`. The API keeps
`value` and `count` and adds `userCount` and `autoCount` on owner tag facets only.
`count` is the deduplicated union, not necessarily the sum: an item may match both
sources, particularly through parent expansion. Each split count equals a search
for that row with the corresponding source and the same trash scope. Hover over
the count to see the total. Ordinary tag-edit responses retain this unified list.

Anonymous and share viewers keep exact User-tag matching and existing facet JSON;
they receive no Auto-tag counts, alias expansion or parent expansion. Source
expressions are refused with 403, including under NOT/OR and when a logged-in owner
browses through a share link. Metadata-hidden shares still have empty facets.

New owner tag queries read a derived snapshot, without contacting the sidecar.
Disabled photos, deleted records, videos and rows from an old model, vocabulary or
thumbnail revision cannot contribute Auto tags. Existing selection snapshots keep
their membership. No authored tables, V6 records or deployment settings change.

Local verification is part of the audience HTTP suite:

```sh
cd gallery-backend
cargo test --offline --locked --profile dev-release --bin urocissa audience_http_tests -- --test-threads=1
```

The gate checks all sort orders, hidden tags, aliases, parents, provenance and
facet/search count reconciliation in all/non-trashed/trashed scopes. It compares
visitor facet, row and scrollbar bytes before and after derived changes, plus
returned metadata after normalizing only existing timestamped capability tokens
and unordered tag/album sets. The pure engine's existing cap/hierarchy tests cover
how displayed and hidden placements are produced. Production operations and the
opt-in hybrid-scroll gate are outside this change.
