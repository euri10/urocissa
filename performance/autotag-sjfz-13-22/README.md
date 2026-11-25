# Full-library Auto tag recalculation campaign

- Target: full materialization latency, lower is better; preserve every prediction,
  authored decision, failure behavior, and the existing memory bound.
- Original integration revision: `7f52e46dbf2486a3b8e1a0d360db91e3eafb343d`;
  tracked and untracked source diff empty. The ignored frontend build output is
  linked from the main checkout for the existing backend test that serves assets.
- Baseline native backend: 353 passed, 8 ignored, serial, Rust 1.97.1. An initial
  isolated-checkout run failed solely because generated frontend assets were absent;
  the unmodified suite passed after linking existing build output.
- Initial candidate-pass cap: **3**; consumed: **4** after extending the campaign
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
