# syntax=docker/dockerfile:1
# gallery-tagger image. Build context: this directory (gallery-tagger/).
#
#   docker build -t gallery-tagger gallery-tagger
#
# Stages: export (pinned weights -> OpenVINO IR) -> build (Rust) -> test (CPU
# parity against the Hugging Face reference; the runtime stage depends on its
# marker, so an image cannot be produced unless parity passes) -> runtime.
ARG RUST_IMAGE=rust:1.99.1-bookworm@sha256:48b7f435b8fc6378ce43b44f15e3a47fd8919b2825f835c39bd4bd3da50af5b8
ARG PYTHON_IMAGE=python:3.12-slim-bookworm@sha256:935c3c5b12357a7e10caa9c1e6ca5692344d97cf229832ef584be2cec9fc03df
ARG RUNTIME_IMAGE=ubuntu:24.04@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852

FROM ${PYTHON_IMAGE} AS export
ARG MODEL_VARIANT=siglip2-base-patch16-224
ENV PIP_NO_CACHE_DIR=1 PIP_DISABLE_PIP_VERSION_CHECK=1 HF_HUB_DISABLE_TELEMETRY=1
COPY tools/requirements.txt /export/tools/requirements.txt
RUN pip install --require-hashes --only-binary :all: --no-deps -r /export/tools/requirements.txt
COPY model.lock.json /export/model.lock.json
COPY tools/common.py tools/export_openvino.py /export/tools/
# Downloads the pinned revision and verifies every file's sha256 before use.
RUN python -I /export/tools/export_openvino.py --variant "${MODEL_VARIANT}" --out /model \
 && rm -rf /root/.cache/huggingface
# Reuse the hash-pinned OpenVINO wheel's runtime: IR frontend, CPU and GPU
# plugins only (no other frontends, NPU, AUTO or HETERO).
RUN set -eu; src=/usr/local/lib/python3.12/site-packages/openvino/libs; mkdir -p /openvino/lib; \
    cd "$src"; \
    cp -a libopenvino.so.* libopenvino_c.so.* libopenvino_ir_frontend.so.* \
          libopenvino_intel_cpu_plugin.so libopenvino_intel_gpu_plugin.so cache.json \
          libtbb.so.* libtbbmalloc.so.* libhwloc.so.* /openvino/lib/; \
    cd /openvino/lib; ln -s libopenvino_c.so.* libopenvino_c.so

FROM ${RUST_IMAGE} AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
COPY tests tests
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release --bin gallery-tagger -j 2 \
 && cp target/release/gallery-tagger /gallery-tagger

FROM build AS test
COPY --from=export /model /model
COPY --from=export /openvino/lib /opt/openvino/lib
ENV LD_LIBRARY_PATH=/opt/openvino/lib TAGGER_MODEL_DIR=/model TAGGER_DEVICES=CPU TAGGER_CPU_PRECISION=f32
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo test --locked --release -j 2 -- --include-ignored --skip latency_and_memory \
 && touch /parity-passed

FROM ${RUNTIME_IMAGE} AS runtime
# Intel GPU compute runtime (OpenCL) for the iGPU; CPU inference needs nothing.
RUN apt-get update \
 && apt-get install -y --no-install-recommends intel-opencl-icd=23.43.27642.40-1ubuntu3 ocl-icd-libopencl1 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=export /openvino/lib /opt/openvino/lib
COPY --from=export /model /opt/gallery-tagger/model
COPY --from=build /gallery-tagger /usr/local/bin/gallery-tagger
COPY --from=test /parity-passed /opt/gallery-tagger/parity-passed
ENV LD_LIBRARY_PATH=/opt/openvino/lib \
    TAGGER_MODEL_DIR=/opt/gallery-tagger/model \
    TAGGER_LISTEN=0.0.0.0:5674
USER 1000:1000
EXPOSE 5674
HEALTHCHECK --interval=15s --timeout=5s --start-period=120s --retries=3 CMD ["/usr/local/bin/gallery-tagger", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/gallery-tagger"]
