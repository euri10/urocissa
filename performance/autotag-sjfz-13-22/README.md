# Full-library Auto tag recalculation campaign

- Target: full materialization latency, lower is better; preserve every prediction,
  authored decision, failure behavior, and the existing memory bound.
- Original integration revision: `7f52e46dbf2486a3b8e1a0d360db91e3eafb343d`;
  tracked and untracked source diff empty. The ignored frontend build output is
  linked from the main checkout for the existing backend test that serves assets.
- Baseline native backend: 353 passed, 8 ignored, serial, Rust 1.97.1. An initial
  isolated-checkout run failed solely because generated frontend assets were absent;
  the unmodified suite passed after linking existing build output.
- Maximum candidate passes: **3**. Consumed: **3**. Baseline harness development
  and read-only profiling do not consume a candidate pass.
- Workload: deterministic seed 23, normalized synthetic 768-dimensional vectors,
  the embedded vocabulary and reference heads, and synthetic feedback exercising
  zero-shot, reference, learned, and example modes. No photo data or personal
  model output is used. Run 6,410 photos and 8,000 for headroom.
- Metric: complete recalculation including gallery input collection, learning,
  embedding reads, scoring, previous-result comparison, and commit. An earlier
  partial-path diagnostic excluded gallery input collection; its raw results are
  retained below but cannot explain or satisfy the production gate.
- Protocol: release build, one warmup then seven full passes; report all samples,
  median, range, CPU/memory limits, and machine conditions. Validate the harness
  at small size before recording the baseline. Set an optional strict median gate
  with `UROCISSA_BENCH_MAX_MS` on an appropriately isolated reference host.
- Minimum useful median gain: 10% at each population. Noise threshold: twice
  that population's baseline max-minus-min range (50.724 ms at 6,410 and 54.548 ms
  at 8,000). Candidate gains must exceed both, without correctness or measured
  resource regression; production must separately observe a complete pass <1 s.

Reproduce in an isolated test process on the target class of host. The optional
`UROCISSA_BENCH_MAX_MS=1000` enforces the median gate; omit it to collect a
failing baseline. Set `UROCISSA_BENCH_PHOTOS=8000` for the headroom case.

```sh
RAYON_NUM_THREADS=2 UROCISSA_BENCH_PHOTOS=6410 UROCISSA_BENCH_SAMPLES=7 \
  UROCISSA_BENCH_PROFILE=1 cargo test --locked --release \
  --manifest-path gallery-backend/Cargo.toml full_library_benchmark -- \
  --ignored --nocapture --test-threads=1
RAYON_NUM_THREADS=2 cargo test --locked --release \
  --manifest-path gallery-backend/Cargo.toml repeated_feedback_memory -- \
  --ignored --nocapture --test-threads=1
```

## Exploratory partial-path baseline (not the acceptance gate)

N100 host, 2 pinned cores (2,3), nice 15, `RAYON_NUM_THREADS=2`, Rust 1.97.1,
`cargo test --release` with the repository's LTO profile, synthetic scratch on
`/var/tmp` (ext4), no gallery data. Test binary SHA-256
`b79de45804277fd66646c34fa505b0936e015dee31a48eba4959f5ef3bf088bb`.
One warmup and seven samples per population:

| Photos | Warmup ms | Samples ms | Median ms | Range ms |
| --- | ---: | --- | ---: | --- |
| 6,410 | 303.088 | 313.788, 313.197, 310.736, 311.332, 319.379, 310.111, 309.968 | 311.332 | 309.968–319.379 |
| 8,000 | 374.328 | 391.950, 387.185, 393.221, 384.798, 399.786, 384.957, 392.206 | 391.950 | 384.798–399.786 |

One reconstructed 6,410-photo attribution: learning 15.030 ms, embedding reads
27.832 ms, scoring 217.336 ms, previous rows and result construction 21.415 ms,
commit 24.890 ms, total 306.969 ms. The 8,000-photo attribution was 15.838,
38.822, 278.982, 29.890 and 31.328 ms, total 395.724 ms, respectively.
These estimates do not include the full `gallery_inputs` and lock path. They
are not comparable directly with the live complete pass of 2,078 ms; no
candidate or gain threshold is accepted from them.

A bounded read-only observation of owner tag facets found 43 nonzero User-tag
facets, total 1,411 facet memberships and maximum 284. Facets include inherited
membership, so these are not all training examples. Four Concept Review reads,
selected on-host by facet count and reduced on-host to counts/modes only, found
one example matcher with 29 positives/8 negatives and three learned classifiers
with 273/113, 93/10 and 127/21 positives/negatives. No labels, photo identifiers,
photos, scores, or raw responses were exported. The full-path fixture uses
synthetic Concepts and vectors with these classifier sizes; this improves the
workload approximation without claiming to reproduce the owner's data.

An intermediate full-path smoke on the development workstation with the smaller
feedback set produced 422.853 ms warmup and 413.052 ms for one timed 6,410-photo
pass. The separate input snapshot took 4.274 ms; reconstructed materialization
attribution was learning 15.189 ms, embedding reads 50.307 ms, scoring 254.622 ms,
previous rows 33.340 ms, commit 27.705 ms, total 382.609 ms. CPU contention was
not controlled and this is a harness diagnostic, not a comparable baseline.
The expanded full-path harness also passed its 800-photo debug smoke with all
four learned modes and its example mode asserted.

The existing release-mode 6,500-photo/30-feedback memory regression also passed
on the development machine (`RAYON_NUM_THREADS=2`): initial RSS 44,084 KiB;
rounds 0–29 KiB: 42,776, 62,432, 62,320, 62,480, 62,480, 62,596, 62,596,
63,284, 63,284, 63,444, 63,444, 64,136, 62,164, 62,152, 62,152, 63,384,
63,384, 61,812, 61,812, 61,812, 61,812, 63,096, 63,096, 63,096, 63,096,
63,096, 63,096, 63,096, 63,096, 63,096. Maximum 64,136 KiB, gate 131,072 KiB.

## Expanded synthetic full-path baseline

Same N100/2 pinned cores/nice 15/2 Rayon threads/release LTO/ext4 protocol.
Binary SHA-256 `3e6a06e94cea74a1231b2e699a5bd4e0b3a0a809ca672a63634d686b1d787026`;
tracked harness diff SHA-256
`34dafc5c2fa688d6ee77b197c3f05649196a1e2a55ee4e09c8b30cb55a5b30ec`.
The true `recalculate()` entry point includes the synthetic TREE snapshot and
locks, not just `materialize()`. One warmup and seven timed passes:

| Photos | Warmup ms | Samples ms | Median ms | Range ms |
| --- | ---: | --- | ---: | --- |
| 6,410 | 413.789 | 426.500, 425.729, 444.582, 420.775, 421.185, 421.587, 422.233 | 422.233 | 420.775–444.582 |
| 8,000 | 488.344 | 511.712, 508.055, 502.675, 503.548, 502.701, 515.451, 521.270 | 508.055 | 502.675–521.270 |

One reconstructed 6,410-photo profile: input 1.591 ms; learning 104.155 ms,
embedding reads 27.971 ms, scoring 231.690 ms, previous rows and result
construction 22.298 ms, commit 27.911 ms, reconstructed materialization total
414.516 ms. The 8,000-photo profile: input 2.235 ms; learning 104.290 ms,
embedding reads 36.367 ms, scoring 289.031 ms, previous rows 29.407 ms, commit
35.037 ms, materialization total 494.718 ms. This fixture still substantially
underestimates the live 2,078 ms complete pass. No candidate has been applied;
investigation continues before defining a comparison threshold.

Four more bounded read-only Concept Review summaries, selected by the next
highest User-facet counts and reduced on-host to counts/modes, found two
similarity matchers with 68/9 and 59/29 positive/negative examples, plus two
learned classifiers with 65/18 and 51/7. The preceding 422 ms fixture included
only the 29/8 example matcher, so it omitted substantial per-photo similarity
work. The next fixture adds equivalent synthetic example counts and one learned
classifier; the 51/7 case was already represented. These anonymous aggregates
do not establish the complete private workload shape.

## Representative full-path baseline and candidate 1

N100/2 pinned cores/nice 15/2 Rayon threads/release LTO/ext4, one warmup and
seven timed full passes. Binary SHA-256
`245f21aaf2c2e78c70f464c609bdf6a2276589e3d92d5aa78786444e38d71fe1`;
tracked baseline harness diff SHA-256
`ed7eaa225caed3de6012c53ebef68628fe0b53f30e92a9ec955e510d3820acb9`.

| Photos | Warmup ms | Samples ms | Median ms | Range ms |
| --- | ---: | --- | ---: | --- |
| 6,410 | 1141.078 | 1158.954, 1150.856, 1170.806, 1145.444, 1153.775, 1162.066, 1145.493 | 1153.775 | 1145.444–1170.806 |
| 8,000 | 1391.024 | 1419.980, 1417.689, 1432.097, 1404.823, 1412.470, 1416.144, 1413.795 | 1416.144 | 1404.823–1432.097 |

One reconstructed 6,410-photo profile: input 1.685 ms; learning 133.319 ms,
embedding reads 28.527 ms, scoring 926.849 ms, previous rows 23.921 ms,
commit 33.654 ms, materialization total 1146.793 ms. At 8,000: input 2.367 ms;
learning 133.941 ms, reads 37.147 ms, scoring 1157.269 ms, previous rows
32.068 ms, commit 36.184 ms, materialization total 1397.240 ms.

Pass 1 hypothesis: the 256-photo storage batch feeds `tag_library` exactly one
256-photo Rayon chunk, serializing its dominant scoring work. Increase only the
bounded storage batch to 512 so two existing engine chunks can run concurrently;
scoring, ordering and authored state remain unchanged. Starting state is original
revision `7f52e46d` plus the benchmark diff above and this ledger. Rollback is a
targeted reversal of this batch-size/comment change and its mirrored profiling
chunk, preserving the benchmark and all unrelated work. Candidate production
patch SHA-256 `28a983fa0ab613ee40f9e79b0edf06bbca3533f1d5cf37c7e54661ba32f8062d`.
The focused 513-photo chunk-boundary test passed. The first full debug run had
352 passed, one failure, nine ignored: the separate semantic-search HTTP gate's
child failed, but its detailed assertion was truncated. It does not exercise
Auto tag materialization. Its unchanged isolated rerun passed at 676.759 ms
semantic and 681.584 ms similar, both below its 1 s debug gate. Heavy concurrent
workstation load was present but is not a proven cause. A full rerun and clippy
were required before measuring or accepting this candidate. The full serial rerun
passed 353 tests with nine ignored; its semantic/similar gates were 831.572 and
879.260 ms. Clippy completed successfully with existing warnings and one new
`too_many_lines` warning for the deliberately self-contained benchmark fixture;
the warning was not suppressed. The 30-decision memory gate passed in all runs
(128 MiB ceiling). Repeated on the same workstation with identical shaped-test
binaries, baseline maximum RSS samples were 66,424, 67,672, 66,508 KiB; candidate
samples were 68,628, 69,204, 68,508 KiB. Medians differ by +2,120 KiB (+3.2%),
with no overlap in these three runs; this is a potential resource regression
despite ample headroom. Raw memory-gate evidence is in `candidate-1-memory.txt`.

Candidate 1 binary SHA-256
`a11be9d3739e42d382a3b5e01f9367e91062dc42a8c537e4359da4096178f3b0`:

| Photos | Warmup ms | Samples ms | Median ms | Range ms | Median gain |
| --- | ---: | --- | ---: | --- | ---: |
| 6,410 | 704.981 | 714.142, 716.694, 712.035, 712.189, 709.824, 751.613, 731.820 | 714.142 | 709.824–751.613 | 439.633 ms (38.1%) |
| 8,000 | 837.368 | 874.783, 879.588, 855.970, 849.295, 848.337, 850.114, 902.725 | 855.970 | 848.337–902.725 | 560.174 ms (39.6%) |

The 6,410-photo profile was input 1.669 ms, learning 135.079 ms, reads 27.435 ms,
scoring 487.947 ms, previous rows 24.184 ms, commit 30.725 ms and reconstructed
materialization 706.015 ms. At 8,000: 2.402, 133.929, 36.256, 601.476, 32.769,
37.633 and 842.868 ms. Both gains exceed the predeclared noise and usefulness
thresholds, but the repeated RSS increase is a measured resource regression.
**Reject pass 1**, restore only its two 512-photo chunk changes, and retain this
evidence. The restored production source must match the baseline exactly.

Pass 2 hypothesis: split a short engine batch into two Rayon chunks while keeping
the existing 256-photo storage buffer and larger engine batches unchanged. This
should parallelize the measured scoring hotspot without doubling input storage or
altering the previously profiled large-vocabulary engine path. Starting state is
the restored baseline production code plus the same benchmark and this ledger.
Rollback is a targeted reversal of the engine chunk-selection change; no other
source or benchmark changes are part of this candidate.
The candidate preserves the original 256-photo engine chunk for inputs larger
than that and for a single-worker Rayon pool. Candidate production patch SHA-256
`274581a0ca2817be7cf8db7c525bf3a350e0409a0c42b9cef6d319e7e1ae14f6`.
Format check and clippy passed (existing warnings and the fixture-length warning
remain). The full serial backend suite passed 353 tests with nine ignored;
the independent semantic/similar HTTP gate measured 713.689/693.560 ms.

Candidate 2 binary SHA-256
`47fca652e2df098a5c2619ac02cdc77a2979e219c35f778aaaf49838346bd659`:

| Photos | Warmup ms | Samples ms | Median ms | Range ms | Median gain |
| --- | ---: | --- | ---: | --- | ---: |
| 6,410 | 689.762 | 702.617, 696.770, 728.219, 704.308, 694.815, 699.498, 694.400 | 699.498 | 694.400–728.219 | 454.277 ms (39.4%) |
| 8,000 | 841.786 | 854.571, 849.100, 840.538, 872.033, 841.282, 840.126, 841.152 | 841.282 | 840.126–872.033 | 574.862 ms (40.6%) |

The 6,410-photo profile was input 1.768 ms, learning 135.342 ms, reads 28.439 ms,
scoring 473.148 ms, previous rows 24.712 ms, commit 30.586 ms and materialization
692.779 ms. At 8,000: 2.448, 133.744, 37.022, 595.218, 33.190, 37.213 and
837.089 ms. Five alternating runs of the existing memory gate on the development
host produced baseline peaks 64,584, 63,728, 64,240, 64,612, 63,900 KiB and
candidate peaks 66,492, 65,908, 66,028, 66,708, 66,540 KiB. Medians are
64,240 and 66,492 KiB (+2,252 KiB, +3.5%), with no sample overlap. The candidate
meets the latency thresholds but has a measured resource regression. **Reject
pass 2** and restore only the engine chunk-selection change.

Pass 3 hypothesis: keep the original packed input and matrix multiplication,
release that input scratch once the dot products are ready, and parallelize only
independent per-photo classification for the short batches used by materialization.
The single-photo and larger-batch paths remain unchanged. This avoids the second
packed scoring buffer while retaining identical dot products and result ordering.
Starting state is the restored baseline production code plus the benchmark and
this ledger. Rollback is a targeted reversal of the engine scoring change alone.
Candidate production patch SHA-256
`597bcaf67249090d28aef89064497f817b2545c018ee78d6d1a58135d5838a88`.
Format and clippy passed (pre-existing warnings and the benchmark fixture-length
warning remain). The full serial backend suite passed 353 tests with nine ignored;
the independent semantic/similar HTTP gate measured 626.121/650.106 ms.

Candidate 3 binary SHA-256
`70b5895efc03f0798322c1cfb77297754d928d8442e69c4b058c4cad16f57600`:

| Photos | Warmup ms | Samples ms | Median ms | Range ms | Median gain |
| --- | ---: | --- | ---: | --- | ---: |
| 6,410 | 700.413 | 724.187, 729.498, 711.393, 722.770, 706.026, 719.909, 717.205 | 719.909 | 706.026–729.498 | 433.866 ms (37.6%) |
| 8,000 | 839.164 | 868.115, 869.738, 851.440, 854.329, 872.347, 852.106, 857.992 | 857.992 | 851.440–872.347 | 558.152 ms (39.4%) |

The 6,410-photo profile was input 1.666 ms, learning 134.264 ms, reads 28.688 ms,
scoring 498.541 ms, previous rows 24.482 ms, commit 30.660 ms and materialization
717.163 ms. At 8,000: 2.371, 132.596, 36.245, 608.406, 31.971, 37.160 and
847.013 ms. Both runs passed `UROCISSA_BENCH_MAX_MS=1000`.

Fifteen alternating runs of the same 6,500-photo/30-feedback memory gate on the
development workstation, in KiB, were:

- Baseline: 63748, 65712, 63632, 64880, 63560, 64072, 66064, 64164, 64468,
  64248, 64816, 66464, 64848, 64108, 64520.
- Candidate: 66632, 64116, 64152, 65536, 63700, 67652, 64916, 66120, 66064,
  64500, 66656, 65684, 63852, 64208, 64480.

Baseline median/range is 64,468/63,560–66,464 KiB; candidate is
64,916/63,700–67,652 KiB. The +448 KiB median difference is well within the
2,904 KiB baseline spread; paired differences range -1,596 to +3,580 KiB (median
+252 KiB; candidate higher in ten runs, lower in five). This test detects no
distinguishable resource regression, but does not prove equal memory use. All
passes remained well below the existing 131,072 KiB gate.

**Keep pass 3:** both median gains exceed their 10% usefulness and predeclared
noise thresholds, all applicable correctness gates pass, and no memory increase
is distinguishable from the paired trial noise. The three-pass campaign is
complete; no further optimization candidate is claimed here. The private
production workload is different, and a real current-model full-pass timing
strictly below 1,000 ms remains required before closing the issue.
