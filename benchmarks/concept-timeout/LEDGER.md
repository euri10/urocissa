# Concept inference timeout investigation

Target: local worker inference on gallery-sized images; lower latency without
weakening privacy, validation or reviewed-tag semantics. No production photos
or data are used. Base app `b3aad28c74b695812c947c8d8ebaaf69a45c2de8`;
clean isolated worktree before this campaign. Maximum candidate passes: **3**;
consumed: **1**. Prior live observation: one owner-selected photo returned 503
in 121.427 s while the worker continued to use two CPUs. This is not a quality
benchmark or a reusable production workload.

Workload: public repository `cat.jpg`, resized to 1920 x 1473 to exercise the
gallery viewer scale. Exact current worker image
`sha256:18f39cf8a771989ef13cfbf9f2bf52e7bd9b06e47737d714243d7caa76e2fdbc`,
CPU-only, network none, 2-CPU quota, 4-GiB limit, 4 PyTorch threads, deterministic
generation with 96 max new tokens; AMD Ryzen AI 7 PRO 350 workstation, not the
production Intel N100. Python in image, PyTorch 2.7.1+cpu, Transformers 4.53.3,
Pillow 13.0.0. One warmup, then three samples per configuration. Record per-sample
prepare and generation timings, filtered labels and peak RSS; compare median and
range. Minimum useful speedup: 20% and more than twice the baseline sample range;
retain only if correctness/privacy checks pass and resource use does not regress.
The public fixture can catch obvious loss such as failing to suggest cat, not
establish general photo quality.

Baseline characterization: existing worker tests 3/3 pass. Processor inspection
shows the default `Idefics3ImageProcessor` uses 13 image patches and 890 input
tokens for both a 512 x 384 image and a 1920 x 1440 image. Setting its existing
`do_image_splitting` property to false produces one 512-pixel patch and 95 input
tokens. This is a hypothesis, not yet a speed or quality result. Production
photo details are not copied into benchmark artifacts.

| Pass | Hypothesis / lever | Samples | Median / range | Gates | Decision |
| --- | --- | --- | --- | --- | --- |
| baseline | default image splitting | `smoke.json`, `baseline.json` | 31.910 s; range 31.290–31.922 s | worker tests 3/3 | 17 patches, labels cat/window, peak RSS 3,406,048 KiB |
| 1 | disable processor image splitting | `candidate-one-patch.json` | 3.592 s; range 3.422–3.759 s | worker tests 4/4; original test fails before change | keep: 88.7% lower median, same cat/window labels, peak RSS 3,336,368 KiB |

Baseline protocol: the tracked `inference_bench.py` ran inside the immutable
worker image with the above resource/security settings, mounting this clean
worktree read-only and only this campaign directory writable. Warmup 31.888 s;
samples 31.290, 31.922 and 31.910 s. The initial 31.384 s smoke run verified
the harness. Gain must exceed 20% and 1.264 seconds (twice the baseline range).
Candidate samples: 3.759, 3.422 and 3.592 s; warmup 3.490 s. Candidate worker
diff SHA-256 at measurement: `0bfb94aadcaf770dce28eed381b97e6783d0f5fb23c1bdce92cedd721a0d0796`.
The harness candidate branch was updated to call the actual worker configuration
function; its baseline execution path and workload were unchanged. This fixture
cannot establish small-object quality, and these are not Intel N100 timings.

Exact baseline command (replace `baseline.json` with
`candidate-one-patch.json` and add `--single-patch` for the candidate):

```sh
docker run --rm --name concept-bench-base-b29 --network none --read-only --tmpfs /tmp:size=128m,mode=1777,noexec,nosuid --cpus 2 --memory 4g --pids-limit 128 --cap-drop ALL --security-opt no-new-privileges:true --user 1000:1000 --mount type=bind,src=/home/lotso/code/urocissa-concept-timeout-b29,dst=/src,readonly --mount type=bind,src=/home/lotso/code/urocissa-concept-timeout-b29/benchmarks/concept-timeout,dst=/results --entrypoint python registry.gitlab.bartab.fr/infra/urocissa/concept-worker@sha256:18f39cf8a771989ef13cfbf9f2bf52e7bd9b06e47737d714243d7caa76e2fdbc /src/benchmarks/concept-timeout/inference_bench.py --samples 3 --warmups 1 --output /results/baseline.json
```

Supplementary single-run public stork fixture (not a statistical benchmark):
`stork-baseline.json` took 30.571 s, `stork-candidate.json` 3.124 s; both
returned `bird nest`, `tree`, `sky`. The benchmark's `--fixture` option was
added after the original cat comparison; its default cat path is unchanged.
These two public fixtures cannot establish general precision or coverage.

Separate correctness fix, not an optimization pass: bound generation between
tokens with a 90-second guard and detect a closed client. An already-abandoned
queued request is rejected before generation; a cancelled generation returns
no partial suggestions. The guard is cooperative: it cannot interrupt an
individual PyTorch operation already executing. Local actual-image socket test
with an abandoned public-fixture request then a second request succeeded in
4.052 s (`worker-integration.json`); worker remained networkless and isolated. Eight worker tests cover the
guard, stale requests, partial-output rejection, socket lease and filtering.
The complete concept-tools suite passed 20 tests. The frontend passed type
check, 370 unit tests, lint and build; local simulated failure was checked in
light/dark desktop and 390px desktop/touch layouts without horizontal overflow.
No optional hybrid virtual-scroll gate was requested or run.

Stop reason: one evidence-backed optimization passed the predeclared threshold;
additional speculative candidates are not needed. Production N100 timing,
owner-photo quality and a true hard bound on a single stuck PyTorch operation
remain unverified. No production photo was used in these benchmarks.
