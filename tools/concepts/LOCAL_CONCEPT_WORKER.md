# Experimental local concept worker

This optional prototype answers an owner-initiated request for one photo. It is
not enabled by default and is not a deployment recipe. The gallery sends a
bounded JPEG thumbnail over a local Unix socket. The worker returns at most
eight untrusted labels; nothing is saved until the owner makes a decision.
Confirmed labels become ordinary User tags and may therefore be public.

For an isolated development test, prepare a local Python environment with
PyTorch, Pillow and Transformers and a previously downloaded, reviewed model.
Then run the worker in a network namespace with no network access, using an
absolute socket path in a private directory accessible to the gallery process:

```sh
unshare --user --map-root-user --net \
  /path/to/venv/bin/python tools/concepts/local_vlm_worker.py \
  --model-dir /path/to/local/model --socket /path/to/private/concept.sock
```

Set `UROCISSA_CONCEPT_SOCKET` for the development gallery process to that
socket path. The worker holds a private lock, refuses an active or untrusted
socket, recovers an owned stale socket, and loads local files only. Those measures and the offline library
flags are not a substitute for OS-enforced network isolation; configure a
networkless container or equivalent before processing personal photos. A
shared socket directory and process identity must also prevent other local
users from impersonating the worker or reading photo thumbnails.

For a local container build, supply a directory containing exactly the public
model files matching `model.sha256`. The named model context is embedded in the
image; the Dockerfile verifies every hash. The ordinary build context excludes
browser artifacts and unrelated files. Building downloads the pinned base and
hash-locked Python wheels, but runtime must have no network access:

```sh
docker buildx build --load --platform linux/amd64 \
  --build-context model=/path/to/checked/model \
  -f tools/concepts/Dockerfile -t urocissa-concept-worker:local-review .
```

Run local tests with `--network none`, read-only root filesystem, dropped
capabilities, no-new-privileges, and a private 1000:1000-owned socket-directory
bind. The socket and lock are mode 0700 and 0600 respectively. The local image
is not a published immutable registry artifact or a production deployment.
One isolated public-fixture test with a two-CPU, 4-GiB limit took 38 seconds;
a second following recovery of an actual stale socket took 36 seconds. These
are single workstation observations, not host capacity or quality evidence.

Model suggestions are restricted to the reviewed exact labels and aliases in
`gallery-backend/src/operations/concept_candidates/allowed_labels.txt`; the
backend independently rejects any other worker response. This is deliberately
conservative and can discard useful suggestions. It is not a content or privacy
classifier: even a permitted label may be wrong or sensitive in context. Custom
owner-entered tags are not restricted. The owner must review every candidate.
Do not log photos, suggestions, or personal data. No automated labeling or
feedback is performed.

The processor uses one overview image patch to reduce CPU latency; small
details may be missed. Generation checks for a disconnected caller and a
90-second deadline between generated tokens, and never returns partial results
after cancellation. A single in-progress PyTorch operation cannot be
interrupted by this cooperative guard. See `benchmarks/concept-timeout/LEDGER.md`
for public-fixture measurements and their limits.

One development workstation test used SmolVLM-500M-Instruct revision
`753c1752602b5e739af7a7fa6947a635051897a3` and a 973 MiB weights file with
SHA-256 `d05b567eeaf534e83d375551f068ed57b5f52d37c657197f644af5ef9db091a2`.
Four public fixtures took roughly 14–19 seconds each on its CPU, with peak
resident memory around 3.2 GiB. This is neither a production hardware benchmark
nor evidence of representative photo quality. Verify the model's license,
artifact integrity, runtime dependencies and production-host capacity before
considering deployment. Production deployment and any real-photo pilot need
the repository's normal operational review and approval.
