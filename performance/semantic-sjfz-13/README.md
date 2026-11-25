# Semantic-search latency campaign

Tracking: `infra-urocissa-autotag-sjfz.13`. The target is lower end-to-end
semantic-search latency without changing search behavior, privacy, Suggested
queue behavior, or the full-library Auto-tagging pass. Maximum candidate passes:
3; consumed: 1.

The starting application revision is `1d47586942feb7db681e548770def2922cfc69f2`,
with no tracked or untracked application changes. The production service is
`0dd8bb873cc07575162c3d8e4ce7dc532ad42d49`; the three-CPU, nice-15 Suggested
worker remains active. The earlier natural full pass took 700 ms; before this
campaign the latest natural full pass was 967 ms for 6,410 photos. Both meet the
one-second gate, but the difference makes an unpaired before/after claim unsafe.

The deployment acceptance protocol is `python3 -B acceptance.py
https://photos.sslc.rocks --host --gitlab-owner --samples 5`. It measures five
distinct fixed public prompts per scope, fresh external HTTPS, independently
verified same-socket reuse, and host-origin fresh HTTPS. Repeated runs must be
more than a minute apart; cache state is not forced. This is not an SLO proof.

Before any application changes, the 2026-11-25 13:37 UTC run found fresh
external p50/p95 222.80/240.17 ms, reused 144.33/150.65 ms, and on-host
179.41/198.17 ms. A separate five-sample phase probe recorded only timing and
size data: DNS 32.7–46.0 ms, TCP 4.5–6.4 ms, TLS 21.5–38.8 ms, and external
request-to-headers 171.3–185.3 ms; response transfer and JSON decoding each
remained below 0.1 ms. Direct host-to-gallery requests took 121.0–135.1 ms;
separate tagger text requests took 48.6–51.0 ms and model-info requests
0.5–0.8 ms. These are distinct requests, so their differences are approximate.

## Synthetic baseline and predeclared acceptance

The baseline application is bee-only commit
`df5a29a1f2155ec0373582a973171055ac3d1ae6`; the only relevant dirty source
was the test harness diff with SHA-256
`ec459123df2aa096d8127349dc4b62e7b12d3b21ccbfaef4a96a52d1ac56e588`.
Release/LTO test binary SHA-256:
`45271bc518a67a91ee76795783bd54b031867b57ce059589913a74d854cc4fce`.
Rust 1.97.1, beesslc N100, two pinned cores (2,3), two Rayon threads, nice 15,
three-CPU/nice-15 Suggested worker active. A validated 256-photo smoke preceded
two baseline runs. Each run uses 6,410 synthetic normalized 768-dimensional
vectors, 6,410 synthetic materialized rows with 10% one-tag rows, an in-process
fake text embedder, one warmup and seven full HTTP requests. No real image,
owner label, model output, or production data is used. Component timings are
separate reconstructed operations; request timing exercises the actual route.

The seven measured full-request samples in `host-baseline-1.txt` were 275.213,
286.846, 279.890, 275.330, 282.052, 282.392 and 279.838 ms (median 279.890);
stdout/stderr transport interleaved the last printed line. In
`host-baseline-2.txt` they were 272.368, 279.136, 277.733, 276.028, 269.415,
272.463 and 262.229 ms (median 272.463). One tag-index construction in the
second run was 109.694–126.745 ms; embedding read 22.418–27.451 ms; exact
cosine/validation 13.643–15.175 ms; ranking 0.844–1.202 ms.

For any candidate, require a median gain greater than both 5% of the lower
baseline median (13.624 ms) and twice the pooled baseline range (49.234 ms),
without behavior, privacy, memory, or full-pass regression. This synthetic
fixture cannot prove the external HTTPS target; the documented five-sample
live measurement remains a separate, necessary gate.

## Candidate ledger

| Pass | Hypothesis / lever | Base and candidate | Artifacts | Result | Decision |
| --- | --- | --- | --- | --- | --- |
| 0 | Baseline; no production candidate applied | `df5a29a1` + harness above | `host-baseline-1.txt`, `host-baseline-2.txt`, `profile-baseline.json` | Synthetic medians 279.890 / 272.463 ms | Baseline |
| 1 | Skip owner tag-index construction only when the filter has no Tag or Source expression; reuse the ordinary compiler | Same base and harness; route diff SHA-256 `06bef2279ab4ec8917d9fb00aa6544964ac0c7c877f5195a04a2e1f55b6ece6a`, test binary `3b8a1648fe6cf5dad175b7e91ed507c29062979343df936c6c3253a1e10c37e0` | `host-candidate-1.txt`, `host-candidate-2.txt`, `rss-*.json` | Medians 44.852 / 44.637 ms; at least 227.611 ms gain from the lower baseline | Keep: gain exceeds both thresholds; functional and RSS gates pass |

The exact baseline harness diff is retained in `baseline-harness.patch`. After
the measurements, the test source additionally gained correctness cases for
positive User tags, Source filtering, negation and ordinary Any filtering, and
renamed a local timing variable and corrected a diagnostic population label; the
production route diff and the benchmark workload did not change. The measured
binary precedes those test-only changes.

One 50-ms sampled RSS run per binary observed 145,760 KiB baseline and 106,640
KiB candidate. This is limited comparative evidence, not a production ceiling.
Under the same worker load, one warmup/seven samples of the separate full-library
benchmark gave medians 572.350 ms for the previous `8d1fd3cd` binary, 594.742 ms
for the bee-only binary, and 564.202 ms for the combined candidate; all were
below 700 ms. Bee-only and combined synthetic result fingerprints matched. The
older binary's fingerprint differs because bee behavior intentionally changed.
Raw samples are in `full-pass-previous.txt`, `full-pass-bee.txt`, and
`full-pass-candidate.txt`. A natural production full pass is still necessary.

The candidate passed the native backend suite (369 passed, 13 ignored), the
public bee regression, filter/visibility checks, formatting and ordinary
Clippy. The repository's pre-existing strict `-D warnings` run reports unrelated
lint debt; it is not represented as a pass. Stop after the first successful
candidate: the remaining external HTTPS target cannot be established without
the separately guarded production release.
