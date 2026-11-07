# gallery-tagger

A stateless embedding sidecar for Urocissa auto-tagging. It turns images and
text into L2-normalized SigLIP 2 embeddings with OpenVINO, preferring the
Intel iGPU and falling back to CPU. It holds no vocabulary, tags or user
state: Auto tags, Concepts, Rejections, learning and search all live in the
gallery, which talks to this service through its `Embedder` client.

The service has no authentication. Run it on an internal network only; never
route it publicly.

## HTTP contract (v1)

| Method | Path | Request | Response |
| --- | --- | --- | --- |
| `GET` | `/healthz` | — | `{"status":"ok"}` once models are loaded |
| `GET` | `/v1/info` | — | model identity, scoring constants, device (below) |
| `POST` | `/v1/embed/image` | raw JPEG, PNG or WebP bytes | `{"model_id", "embedding": [f32; 768]}` |
| `POST` | `/v1/embed/text` | `{"texts": ["…"]}`, 1–64 non-blank texts of at most 2,000 characters | `{"model_id", "embeddings": [[f32; 768], …]}` in input order |

Errors are `{"error": "…"}` with 400 (undecodable image, invalid request),
413 (image body over `TAGGER_MAX_IMAGE_BYTES`) or 500 (inference failure).

`/v1/info` returns `model_id`, `variant`, `revision`, `precision`,
`embedding_dim`, `image_size`, `text_max_tokens`, `logit_scale`,
`logit_bias`, `device`, `runtime` and `tagger_version`.

- **`model_id`** identifies the exact weights and export precision, e.g.
  `siglip2-base-patch16-224@75de2d55ec2d/fp16-text-int8`. Embeddings with
  different model IDs are not comparable; a client must re-embed when it
  changes.
- **Scoring**: probability = `sigmoid(dot(image, text) * logit_scale + logit_bias)`.
  `logit_scale` is already exponentiated.
- **Text** is lowercased by the service (SigLIP 2 was trained on lowercased
  text) and truncated to 64 tokens. Clients send it as typed.
- **Images** are decoded with their EXIF orientation applied, alpha dropped,
  resized (bilinear) to a square and normalized. Videos are not supported.
- **`device`** is what OpenVINO reports as executing, e.g. `GPU.0` or `CPU`.

## Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `TAGGER_MODEL_DIR` | `/opt/gallery-tagger/model` | Exported model (manifest, IR, tokenizer) |
| `TAGGER_LISTEN` | `0.0.0.0:5674` | Listen address |
| `TAGGER_DEVICES` | `GPU,CPU` | Devices tried in order. A device is used only if both towers compile and a self-test inference succeeds, so a missing iGPU falls back to CPU at startup |
| `TAGGER_CPU_THREADS` | OpenVINO default | CPU inference threads. Set it to the container's CPU quota, which OpenVINO cannot see |
| `TAGGER_CPU_PRECISION` | OpenVINO default | `f32` or `bf16` (bf16 is chosen automatically only on CPUs that support it) |
| `TAGGER_CACHE_DIR` | unset | OpenVINO model cache, mainly to avoid recompiling GPU kernels on restart; a tmpfs is enough |
| `TAGGER_MAX_IMAGE_BYTES` | 33554432 | Image body limit |
| `RUST_LOG` | `info` | Log filter |

`gallery-tagger healthcheck` exits 0 when the local `/healthz` answers 200,
for container health checks without curl.

For the iGPU, the container needs `/dev/dri/renderD128` and the host's
`render` group (`group_add`). The image ships Intel's OpenCL compute runtime.

## Model provenance

`model.lock.json` pins the Hugging Face repository, revision and sha256 of
every file for each supported variant (Apache-2.0). The Docker build:

1. installs the hash-locked export toolchain (`tools/requirements.txt`);
2. runs `tools/export_openvino.py`, which downloads the pinned revision,
   refuses any file whose sha256 differs, and writes OpenVINO IR: vision
   weights FP16, text weights INT8 (asymmetric, per channel);
3. builds the service and runs the full test suite, including parity against
   the FP32 Hugging Face reference at CPU f32 precision;
4. copies only the OpenVINO IR frontend and CPU/GPU plugins into the runtime
   image. No weights are downloaded at runtime.

Change the variant with `--build-arg MODEL_VARIANT=siglip2-base-patch16-256`.

## Verifying parity

`tests/fixtures/reference-*.json` hold FP32 transformers embeddings and exact
token IDs for the fixtures (see `tests/fixtures/ATTRIBUTION.md`). With an
exported model and OpenVINO libraries on `LD_LIBRARY_PATH`:

```sh
TAGGER_MODEL_DIR=/path/to/model TAGGER_DEVICES=CPU \
  cargo test --release -- --include-ignored --skip latency_and_memory
TAGGER_DEVICES=GPU ...   # the iGPU path, on a host that has one
```

The suite requires exact token IDs, cosine ≥ 0.99 for every image and text,
each fixture photo ranking its own caption first, and bit-identical outputs
across repeated calls and restarts. Regenerate references with
`tools/make_reference.py` and fixtures with `tools/prepare_fixtures.py`.

## Measurements

Dev box (AMD Zen 5), CPU, median of 20, image = decode + inference of a
640 px JPEG, text = one caption. The N100 has no bf16, so the f32 rows are the
relevant ones for its CPU fallback; expect it to be roughly 2–3× slower per
core. Host (N100 iGPU and CPU) numbers are recorded at deployment.

| Variant | Precision | Threads | Image | Text | Peak RSS |
| --- | --- | --- | --- | --- | --- |
| 224 (shipped) | f32 | 1 | 273 ms | 36 ms | 918 MiB |
| 224 (shipped) | f32 | 2 | 195 ms | 29 ms | 1,086 MiB |
| 224, FP16 text tower | f32 | 2 | 200 ms | 78 ms | 1,425 MiB |
| 256, FP16 text tower | f32 | 2 | 268 ms | 75 ms | 1,421 MiB |

Worst parity cosine (shipped 224 artifact): 0.99938 at f32, 0.99941 at bf16;
text tokens match transformers exactly. 224 is the default: 256 costs about
30% more image latency for a small zero-shot gain, and the INT8 text tower
cuts text latency and memory by more than half with no parity loss.
