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
