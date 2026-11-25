# Full-library Auto tag recalculation campaign

- Target: full materialization latency, lower is better; preserve every prediction,
  authored decision, failure behavior, and the existing memory bound.
- Original integration revision: `7f52e46dbf2486a3b8e1a0d360db91e3eafb343d`;
  tracked and untracked source diff empty. The ignored frontend build output is
  linked from the main checkout for the existing backend test that serves assets.
- Baseline native backend: 353 passed, 8 ignored, serial, Rust 1.97.1. An initial
  isolated-checkout run failed solely because generated frontend assets were absent;
  the unmodified suite passed after linking existing build output.
- Initial candidate-pass cap: **3**; consumed: **6** after extending the campaign
  under the owner's explicit instruction to stop only for a real data-loss or
  secret-exposure risk. The prior passes are not reset. Baseline harness
  development and read-only profiling do not consume a candidate pass.
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

## Continued investigation after the live gate

The owner explicitly directed the task to continue unless a real data-loss or
secret-exposure risk blocks it. The three-pass default therefore does not end the
work after deployment: protected job 171619 produced a natural current-model
6,410-photo full pass of 1,208 ms, down from the previous 2,078 ms observation
but still above the strict 1,000 ms gate. The accepted pass 3 is now the deployed
baseline; none of the earlier passes are reset or concealed. Additional profiling
uses synthetic data and must preserve all decisions and the frozen review round.

Pass 3 full-path synthetic baseline medians/ranges remain 719.909/706.026–729.498
ms at 6,410 photos and 857.992/851.440–872.347 ms at 8,000. For the next causal
lever, require at least 10% median improvement over this deployed state at each
size and more than twice its baseline range (46.944/41.814 ms respectively), no
correctness or distinguishable resource regression, and the separate live gate.
The next read-only diagnostic measures positive and rejected-example comparisons
inside the synthetic similarity classifier; no fourth candidate has been applied.

The diagnostic is the ignored `similarity_hotspot_profile` test. On the
development workstation, release/LTO/Rust 1.97.1, seed 23 and 1,100 generated
768-dimensional vectors, it exercises three synthetic example sets sized 29/8,
68/9 and 59/29. It measures the real `nearest` implementation sequentially;
these are CPU attributions, not a production latency or quality measurement.
Each row is positive-comparison ms / rejected-comparison ms / rejected-comparison
ms on photos already below the positive cutoff:

`82.606/23.928/21.385`, `77.795/22.688/20.244`,
`74.741/22.042/19.697`, `82.976/24.977/21.529`,
`80.121/23.407/20.948`. In each sample 2,928 of 3,300 synthetic decisions were
already below the positive cutoff, and 367 fired after the rejection comparison.
This supports exploring a decision-only early rejection while retaining the full
explanation score. The new diagnostic test has no production effect. Its release
binary SHA-256 is `0bcd83497ae5c030b1217d84eb810e99df6b596f038715c2608494b3c46645df`;
its test-source patch SHA-256 is
`be7f45960787133c731a138291c09fdbc6454d202b58b67566a1ec02fee2629a`.
Capture a fresh full-path baseline with this same binary before applying the next
candidate; the added diagnostic changes the test binary even though it is ignored.

Refreshed baseline on the same N100, pinned cores 2/3, nice 15, two Rayon threads,
release/LTO and ext4 scratch, with the diagnostic binary above:

| Photos | Warmup ms | Samples ms | Median ms | Range ms |
| --- | ---: | --- | ---: | --- |
| 6,410 | 698.585 | 712.038, 711.560, 711.445, 718.481, 734.398, 706.435, 707.980 | 711.560 | 706.435–734.398 |
| 8,000 | 842.239 | 890.411, 857.883, 876.396, 854.994, 852.372, 855.515, 852.643 | 855.515 | 852.372–890.411 |

The 6,410-photo reconstructed attribution was input 1.690 ms, learning 131.996,
reads 28.074, scoring 487.409, previous rows 24.655, commit 28.322 and total
700.981 ms; at 8,000: 2.337, 131.939, 36.142, 608.684, 32.452, 37.118 and
847.003 ms. Pass 4 must improve the refreshed median by at least 10% and by more
than twice its refreshed range: 55.926 ms at 6,410 and 76.078 ms at 8,000.

Pass 4 hypothesis: for example-based classification, avoid comparing Rejections
when an upper bound computed from the exact positive score already cannot fire.
Compute the original full probability for explanations and for every firing
decision; use the same sigmoid comparison at the boundary to preserve rounding,
including values that round to exactly 0.5. Starting state is deployed source
`89eb4b2a` plus the ignored diagnostic and this ledger. Rollback is a targeted
reversal of the decision-only classifier/engine change and its new tests.
Production patch SHA-256
`bd2771bbb64860fa3ce806fac248f51b3fb148e3ac8a6b75f79656ea84945013`.
The focused tests compare the shortcut with the complete probability at
threshold-rounding boundaries and on 2,000 deterministic random vector sets;
every firing confidence must match bit-for-bit. Both focused tests pass.
After tightening the threshold assertion to compare bits, the full serial suite
passed 355 tests with 10 ignored; format and clippy passed with the pre-existing
warnings and the unsuppressed benchmark-fixture length warning. The unrelated
semantic/similar HTTP gate also passed in the first full run at 694.253/645.234 ms.

The initial candidate binary
`ce866ed66a9941b66495326bdeddaaf42b4cc4da7232176689f306e9e969355a`
produced 6,410-photo warmup 698.870 ms and samples 729.734, 721.741, 710.315,
707.052, 708.221, 714.331 and 706.431 ms, median 710.315 ms and range
706.431–729.734 ms. Reconstructed input/learning/reads/scoring/previous/commit
were 1.712/141.271/27.709/489.882/24.852/30.642 ms. This did not improve the
711.560 ms baseline. Investigation found that the production `ReferenceFallback`
wrapper used the trait's full-score default rather than forwarding the new fast
decision to its wrapped owner classifier. The intended shortcut was not reached;
the 8,000-photo timing was not run for this diagnostic build.

Seven alternating memory-gate peak RSS trials, KiB, were baseline
62,336, 61,156, 62,556, 63,016, 62,592, 63,324, 64,360 and initial candidate
66,292, 67,492, 67,792, 65,224, 66,824, 65,160, 65,012. The medians differ by
+3,700 KiB with no overlap, although the 128 MiB gate passed. Three further
paired trials gave initial/peak/final KiB:

- Baseline: 42056/62088/59220, 42444/63912/61276, 42252/62296/61792.
- Candidate: 45368/66040/62972, 45352/67348/67032, 45524/65724/65272.

For a separate three-pair diagnostic, synthetic test processes were briefly
paused at their initial measurement and only `/proc/PID/status` RSS aggregates
were read. Baseline anonymous/file KiB: 29020/13476, 29004/13340, 29000/13136;
candidate: 29020/16340, 29012/16568, 28992/16392. Thus most of the total-RSS
difference was file-backed test-binary pages, not anonymous memory; it remains
a real observed test-process RSS difference and does not prove production memory
equivalence. The actual fast path was bypassed in this build.

The first local repair tightened an exact threshold test without suppressing a
lint. The second and final allowed local repair forwards the decision-only hook
through `ReferenceFallback`, retaining the original full-score behavior for its
own public fallback heads. A new routing regression failed before this repair;
the existing fallback test also checks fast and full decisions agree. Rerun all
affected correctness, memory and performance gates before any acceptance.
The combined classifier/engine/learning/reference patch (including its routing
test) SHA-256 is
`01d42794f57b1b2c604040ae93dcb6ec69230fa0756b29103809fe09e4d7d401`.
Both fallback-routing/fallback-equivalence tests and both exact-score tests pass
after the repair.
The full serial suite passed 356 tests with 10 ignored; format and clippy passed
with the same existing and fixture-length warnings. Release-mode routing and
exact-score tests also passed.

Repaired candidate binary SHA-256
`694c7e26ddf4f46196a0758c7ebf335dab17e108f2716b51e8f362e4962101db`:

| Photos | Warmup ms | Samples ms | Median ms | Range ms | Median gain |
| --- | ---: | --- | ---: | --- | ---: |
| 6,410 | 611.858 | 626.524, 622.469, 622.313, 625.183, 672.667, 626.985, 620.414 | 625.183 | 620.414–672.667 | 86.377 ms (12.1%) |
| 8,000 | 732.825 | 773.427, 760.186, 763.639, 746.598, 747.972, 753.424, 774.549 | 760.186 | 746.598–774.549 | 95.329 ms (11.1%) |

The 6,410-photo reconstructed profile was input 1.669 ms, learning 139.993,
reads 28.516, scoring 398.578, previous rows 24.194, commit 30.031 and total
621.877 ms; at 8,000: 2.411, 132.856, 39.138, 516.572, 34.321, 36.059 and
759.698 ms. Both runs passed the strict synthetic median gate, and each gain
exceeds its predeclared threshold.

Seven alternating baseline/repaired-candidate memory trials sampled the
6,500-photo/30-feedback gate. Reported peak RSS KiB:

- Baseline: 62160, 62196, 62256, 62228, 62132, 61976, 61612.
- Candidate: 65444, 65168, 65304, 66004, 65232, 64964, 65004.

Medians are 62,160 and 65,232 KiB (+3,072 KiB). Independently sampled peak
anonymous KiB, in the same order: baseline 50964, 51208, 51308, 51268, 50668,
51012, 50256; candidate 51412, 50792, 51360, 49900, 51448, 50968, 50444.
Medians are 51,012 and 50,968 KiB. Sampled peak file-backed KiB: baseline
13664, 13456, 13416, 13428, 13516, 13432, 13568; candidate 16084, 16428,
15996, 16104, 16228, 16148, 16588. Medians are 13,456 and 16,148 KiB.
Peaks of components need not occur at the same time. All reported total-RSS
peaks remain below 131,072 KiB. The differing test executables obscure whether
the measured total-RSS increase reflects the runtime change or test-code layout;
a same-binary, test-only full-versus-fast control is required before a decision.
The separate Suggested-tier app candidate `32c75e0e` and its model load also
require integration and remeasurement before the issue can be closed.

The same-binary control used a separate temporary worktree, the repaired code,
and a `cfg(test)`-only flag that selects the original full-classification path
or the fast path in the same executable. Both modes passed the existing
chunk-boundary correctness test. Release/LTO control binary SHA-256:
`2f5a60873027d254ecd2fa4ad4c05f46dc3d8fac0e274c4a7e1a0f5089385e49`.
Seven alternating 6,500-photo/30-feedback trials, KiB:

| Path | Reported peak RSS | Sampled peak anonymous | Sampled peak file-backed |
| --- | --- | --- | --- |
| Full | 65332, 64732, 65884, 65048, 64916, 61892, 67952 | 50796, 50340, 50828, 51180, 50764, 49752, 51856 | 17004, 16772, 17108, 16656, 16620, 14896, 16096 |
| Fast | 65712, 66380, 66124, 65788, 65196, 65216, 63348 | 50856, 50228, 50872, 50808, 50828, 50796, 49492 | 16884, 16864, 16460, 17032, 16832, 16472, 14848 |

Total-RSS medians/ranges are 65,048/61,892–67,952 and
65,712/63,348–66,380 KiB; the +664 KiB median difference is within the
6,060 KiB baseline spread. Median anonymous peaks are 50,796 and 50,808 KiB;
median file-backed peaks are 16,656 and 16,832 KiB. Sampled component peaks may
occur at different times. This controlled test finds no distinguishable memory
regression; it does not establish exact memory equivalence in production.

**Keep pass 4 on the 89eb base:** both synthetic latency gains clear the
predeclared thresholds, exact-decision and other correctness gates pass, and
the controlled memory comparison detects no distinguishable increase. This is
not live acceptance: repeat against the integrated Suggested candidate and its
coordinated model load, then require a natural current-model production pass
strictly below 1,000 ms. The first deployment's 1,208 ms observation remains a
failure, not a waiver.

## Pass 5: independent Concept fitting (rejected)

The 6dd integrated deployment completed a natural live 6,410-photo pass in
1,046 ms, still above the strict gate. With one Suggested worker active, the
6dd synthetic medians were 746.033 ms/6,410 and 845.951 ms/8,000. Coverage
candidate 84d3d372 removes the five inactive reference heads and changes its
vocabulary version; its current model and library-scale runtime must be measured
separately. The frozen owner round still describes the prior vocabulary.

An exploratory 84d synthetic run on the development workstation (two pinned
cores, nice 15, one warmup/seven samples) gave 6,410 median 663.102 ms, range
567.211–791.837 ms, and one reconstructed learning phase of 139.549 ms. This
uncontrolled run is only a hotspot diagnostic, not a target-host baseline.
Pass 5 tests one lever: fit independent Concepts concurrently while preserving
the original per-Concept photo order, floating-point operations and results.
Before accepting it, use the final 84d candidate and one natural Suggested
worker on the N100. At each population, record three alternating baseline/
candidate runs, each with one warmup and seven samples, and compare the medians
of their medians. The gain must exceed both 5% and twice the spread between the
three baseline run medians; if background noise masks that effect, collect a
new complete paired series rather than select individual samples. This smaller
minimum is predeclared because the observed live gap is now 4.6%; it does not
retroactively change the 10% thresholds or decisions for prior passes. Exact
single- versus two-thread results, full tests and a controlled memory comparison
remain mandatory. A natural live current-model pass below 1,000 ms is still
the acceptance gate; synthetic results cannot substitute for it.

The exact 84d release-test binary
`c3f6eb494c3cdbc1d8fbe1bbe52b34bde80f83316d362bed2abe7abfb173a3f5`
was measured on the N100 with one natural Suggested backfill worker active:
6,410 warmup 697.987 ms, samples 744.801, 697.060, 701.029, 768.552,
679.777, 667.003, 683.158 (median 697.060); 8,000 warmup 800.956 ms,
samples 825.073, 872.371, 843.936, 829.522, 822.100, 832.292, 809.885
(median 829.522). Both strict synthetic gates pass. The independently observed
natural 84d production pass was 1,013 ms for all 6,410 current-model photos,
which still fails the live gate.

The opt-in test-only serial control and the candidate shared release test binary
`012661c0456223409b406b8d73ceddaaa2a0d6facf92292b679f73bf724dd2cf`.
The one-versus-two-thread regression compared the resulting vocabulary and
serialized learned parameters, including two learned and one personal-example
Concept, reversed input and a duplicate. On the development workstation only,
one exploratory 6,410-photo serial run had median 517.289 ms and a reconstructed
learning phase of 119.469 ms; the parallel run had median 494.635 ms and learning
64.401 ms. Contention and the wide baseline sample range make these diagnostics,
not evidence that pass 5 meets its predeclared acceptance threshold.

Seven alternating same-binary 6,500-photo/30-feedback memory trials, reported
peak RSS KiB, were serial 64268, 65988, 65240, 65456, 64712, 63424, 65328;
parallel 63716, 65628, 65468, 65748, 66764, 68044, 66088. Their medians are
65,240 and 65,748 KiB; the 508 KiB difference is within the 2,564 KiB serial
range. Sampled peak anonymous medians were 48,952 and 49,532 KiB, and all
reported peaks were below the existing 131,072 KiB gate. These measurements do
not prove identical production memory use. This binary preceded a local helper
extraction to address new clippy warnings; the final-binary control follows.

After the helper extraction, the final test binary was
`e7bf445d25e025d337897d49abeafa2637e9f13a2a5f540372ed17d4c01f5840`.
Its seven alternating same-binary memory trials reported peak RSS KiB serial
64632, 64984, 64064, 65800, 64952, 65620, 62976; parallel 66712, 66376,
67576, 65700, 66768, 66372, 65332. Medians are 64,952 and 66,376 KiB;
the 1,424 KiB difference is within the 2,824 KiB serial spread. Sampled peak
anonymous medians were 49,680 and 49,624 KiB, respectively. All reported peaks
passed the existing 131,072 KiB gate; no distinguishable memory regression was
detected at this sampling resolution. The post-extraction full backend suite
passed 364 tests with 11 ignored; format and clippy passed with existing warnings
and none in the modified fitting file.

On beesslc with the single natural Suggested worker, the final same-binary
6,410-photo serial run medians were 666.516, 689.484 and 679.724 ms; the
parallel medians were 659.699, 652.233 and 639.057 ms. The median gain was
27.491 ms, below the predeclared 45.936 ms threshold (twice the serial-median
spread, greater than 5%). The 8,000-photo series was stopped after one serial
run, median 825.018 ms, because the 6,410 gate had already failed. No pass count
or baseline was reset. The fitting optimization was removed; its correctness and
memory tests above are retained as experimental evidence, not shipped code.

## Inactive Concept matrix attribution (no candidate)

The public 82-Concept catalog contains 23 threshold-enabled entries; owner
classifiers can activate additional entries. The current scorer multiplies all
82 vectors per photo even when a Concept is neither zero-shot nor classified.
Before changing it, a synthetic property test checks selected-column dot
products bit-for-bit against the complete matrix across 1–257 rows and seven
column layouts. A separate ignored release test attributes the cost of a full
82-column matrix versus a synthetic 28-column selection without using photos or
model output. These tests are diagnostics, not a production change.

If profiling supports the candidate, the one lever is to omit inactive matrix
columns while retaining every zero-shot or classified Concept, all exact scores,
hierarchy/alias/rejection behavior and explanation semantics. Before accepting,
compare the final test binary's full-versus-selected test-only paths on the N100
with the then-current production worker load. At each of 6,410 and 8,000 photos,
use three alternating pairs of one warmup plus seven full-pass samples and compare
the medians of run medians. Require a gain greater than both 5% and twice the
spread of the three baseline medians. Record the worker version/load and any
changed candidate; a source or runtime change requires a fresh comparison.
The correctness suite must compare decisions and confidence bits across layouts,
including inactive ancestors, and the controlled memory gate must show no
distinguishable regression. A natural live complete current-model pass below
1,000 ms remains mandatory. Pass 5's failed threshold and count are not reset.

The first workstation release attribution, 25 synthetic 256-photo blocks,
gave seven full-matrix samples 8.963, 9.342, 9.621, 9.295, 8.908, 10.340,
9.373 ms (median 9.342) and selected-column samples 3.978, 4.053, 4.134,
3.921, 3.876, 4.299, 4.008 ms (median 4.008). This 5.334 ms difference is
too small on that machine to justify a production rewrite by itself; target
host attribution is still needed. No matrix optimization has been applied.

On beesslc, the same release binary passed the bit-for-bit property test.
The seven full-matrix samples were 23.546, 25.055, 25.477, 22.980, 23.305,
25.445, 22.943 ms (median 23.546); selected-column samples were 11.304,
11.325, 10.820, 9.173, 10.280, 10.034, 9.187 ms (median 10.280). The
13.266 ms stage saving, and even the entire measured 23.546 ms stage, are below
the predeclared approximately 35 ms full-path threshold. Do not implement the
matrix rewrite on this evidence. Profiling alone does not consume a candidate
pass; five candidates have been evaluated, with pass 5 rejected above.

## Bounded similarity prefilter (profiling only)

The earlier similarity attribution showed substantially more time in nearest
positive-example dot products than in the full Concept matrix. A synthetic-only
prototype explores an eight-lane dot-product upper bound for 768 dimensions,
including a conservative error allowance based on the sum of absolute products.
It may rule out a firing result only when that upper bound itself cannot fire;
uncertain/non-finite inputs must fall back to the existing exact dot and every
firing confidence must remain bit-identical. First measure the standalone cost
and test the bound on normalized, cancellation-heavy, extremely scaled and
rounding-boundary inputs. No runtime change is justified by the prototype alone.

If a candidate follows, predeclare the same N100 paired protocol as above:
three alternating full/fast pairs, one warmup/seven samples each, at 6,410 and
8,000 under the final actual worker configuration; require a gain above both 5%
and twice the baseline-median spread at each size. Full decision/score equality,
the existing memory gate, and a natural current-model live pass below 1,000 ms
remain mandatory. The pass-5 rejection and all prior results remain recorded.

The profiling-only release/LTO binary
`fee8d4fa5d417ab37b5dc2253787c927cb73d8aa0cec29d6039e342bdb5b7f75`
ran 171,600 synthetic 768-dimensional products per sample on the development
workstation. Exact-dot samples were 87.814, 85.882, 92.128, 83.783, 96.284 ms
(median 87.814); conservative upper-bound samples were 35.246, 56.790, 37.953,
38.191, 40.464 ms (median 38.191). This component-level improvement does not
establish a full-library gain. The prototype bound covered exact results for
2,500 randomized pairs across extreme reciprocal scales, and non-finite
arithmetic fell back; additional adversarial and end-to-end tests are required.

Candidate pass 6 uses the standard floating-dot bound described by
[Rump, section 4](https://www.tuhh.de/ti3/paper/rump/Ru11.pdf): for 768
single-precision terms, unit roundoff is 2^-24 and gamma_768 is about
0.00004578. Allowing for both summation orders gives less than 0.000092 times
the absolute-product sum before the additional guard for summing that magnitude.
The implementation uses the more conservative `0.001 * (magnitude + 1)`;
the additive term dominates subnormal absolute error, and non-finite values,
absolute-product sums above 2, or other dimensions fall back to the original
exact calculation. This bound is
used only to reject impossible matches; a possible match is scored exactly.
Synthetic tests cover random reciprocal scales, cancellation, near-cutoff
decisions and non-finite fallback; the full benchmark additionally emits a
synthetic-only output fingerprint for exact-versus-bounded comparison.

An exploratory development-workstation 6,410-photo full-path run gave exact
median 546.152 ms and bounded median 311.382 ms with identical synthetic output
fingerprints. These uncontrolled, separate runs support target-host measurement
but are not the predeclared paired comparison or live acceptance evidence.

The parallel Suggested rollout subsequently deployed application ed1e129c and
the same model on a three-CPU/nice-15 worker. Its natural current-model 6,410
photo pass was 1,718 ms. The paired comparison must use this actual load;
synthetic low-priority timing is interpreted comparatively, since it competes
with the low-priority three-core worker. The live `<1 s` gate is unchanged.

The final test executable
`d39c76e2f5aa7e17a4c23b835d9e6cffaa1da8d735e077cfbd3ffe71945b412d`
passed the release learning tests on the N100. Seven alternating same-binary
6,500-photo/30-feedback memory trials reported peak RSS KiB: exact 64832,
63844, 63992, 63488, 65764, 62972, 65356; bounded 65916, 64628, 62584,
65988, 65640, 64796, 63908. Medians were 63,992 and 64,796 KiB, a difference
of 804 KiB within the exact-path range of 2,792 KiB. Sampled peak anonymous
medians were 49,072 and 49,324 KiB. All peaks passed the 131,072 KiB gate; no
distinguishable memory increase was detected at this sampling resolution.

First local repair: while the initial paired run was underway, a review found
that malformed, extremely large inputs might overflow in the original order
even if an alternate order happened to remain finite. Although validated unit
vectors cannot reach that case, the helper now refuses an absolute-product sum
above 2 and falls back to the original dot. The old 6,410 series had exact
medians 1002.455, 1087.533, 1018.699 ms and bounded medians 881.659, 828.806,
811.995 ms (gain 189.893 ms, threshold 170.156 ms); all synthetic fingerprints
matched. The 8,000 series was interrupted because it used the pre-repair binary.
Those results do not accept the repaired candidate: repeat the entire paired
timing, fingerprint and controlled memory gates on the final code without
resetting the candidate count.

The repaired test binary is
`8d1fd3cda1068f626779feb4b196ece2995a81674eeb4fa7edc8986a88c738e5`.
It passed the release learning tests on beesslc and the full backend suite
(368 passed, 13 ignored). Seven alternating same-binary 6,500-photo/30-feedback
memory trials reported peak RSS KiB: exact 65072, 64240, 62768, 64940, 66028,
64636, 66112; bounded 65656, 63728, 65204, 64524, 64532, 65336, 63564.
Medians were 64,940 and 64,532 KiB, within the 3,344 KiB exact-path range;
sampled peak anonymous medians were 48,956 and 48,676 KiB. All reported peaks
passed the 131,072 KiB gate, with no distinguishable memory increase in this
controlled comparison.

The complete repaired-binary paired series ran on beesslc under the deployed
three-core/nice-15 Suggested worker, with the synthetic process pinned to two
cores/nice-15 and two Rayon threads. Each run used one warmup and seven timed
samples. These are comparative measurements, not the live acceptance gate.

| Photos | Mode | Warmups (ms) | Three sets of seven samples (ms) |
| --- | --- | --- | --- |
| 6,410 | exact | 904.019, 993.317, 998.699 | 1068.345, 870.738, 980.361, 915.458, 1009.260, 953.918, 973.327; 946.813, 977.163, 1046.197, 1133.724, 1062.270, 916.858, 1030.677; 1035.647, 963.822, 1047.339, 1063.841, 1063.000, 1061.909, 1113.671 |
| 6,410 | bounded | 531.295, 546.312, 559.611 | 580.891, 523.628, 570.901, 561.718, 571.380, 575.367, 541.363; 576.835, 523.721, 570.225, 571.319, 566.493, 632.483, 562.770; 694.388, 688.623, 653.350, 630.442, 613.975, 623.680, 560.723 |
| 8,000 | exact | 1260.653, 1114.818, 1256.669 | 1275.171, 1263.943, 1172.029, 1079.627, 1229.005, 1412.839, 1370.115; 1186.024, 1391.001, 1127.725, 1321.714, 1158.095, 1273.674, 1244.298; 1246.082, 1310.275, 1288.940, 1245.150, 1246.873, 1185.071, 1311.282 |
| 8,000 | bounded | 590.691, 729.100, 674.562 | 667.783, 636.550, 651.092, 576.509, 632.397, 638.436, 599.605; 648.697, 768.334, 601.298, 624.930, 637.409, 672.480, 648.992; 683.807, 681.495, 662.583, 664.902, 690.707, 672.106, 661.501 |

At 6,410, exact-run medians were 973.327, 1030.677 and 1061.909 ms;
bounded-run medians were 570.901, 570.225 and 630.442 ms. The 459.776 ms
median-of-medians gain exceeds the predeclared 177.164 ms threshold. At 8,000,
exact medians were 1263.943, 1244.298 and 1246.873 ms; bounded medians were
636.550, 648.697 and 672.106 ms. The 598.176 ms gain exceeds the 62.344 ms
threshold. Every exact/bounded run at each size had the same synthetic output
fingerprint: `62635b3a0eeadd646dcb59f6d94129e855f4ffe73e3145984aba736238736a9d`
at 6,410 and `dcbe2d3804cc72d7b3f573559f253d36ad259f91b887f81878ee678c8d7e1116`
at 8,000. The natural full-library live timing and deployment verification are
still required.
