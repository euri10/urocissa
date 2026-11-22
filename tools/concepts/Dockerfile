FROM python:3.12-slim-bookworm@sha256:935c3c5b12357a7e10caa9c1e6ca5692344d97cf229832ef584be2cec9fc03df

ENV PIP_DISABLE_PIP_VERSION_CHECK=1 PIP_NO_CACHE_DIR=1 \
    HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 HF_HUB_DISABLE_TELEMETRY=1 \
    PYTHONDONTWRITEBYTECODE=1
WORKDIR /app
COPY tools/concepts/worker-requirements.txt /app/worker-requirements.txt
RUN pip install --require-hashes --only-binary=:all: --no-deps \
    --index-url https://download.pytorch.org/whl/cpu \
    --extra-index-url https://pypi.org/simple -r /app/worker-requirements.txt

COPY --from=model / /opt/model/
COPY tools/concepts/model.sha256 /app/model.sha256
RUN cd /opt/model && sha256sum -c /app/model.sha256 \
    && test "$(find . -mindepth 1 -maxdepth 1 | wc -l)" -eq 12
COPY tools/concepts/local_vlm_worker.py /app/tools/concepts/local_vlm_worker.py
COPY gallery-backend/src/operations/concept_candidates/allowed_labels.txt /app/gallery-backend/src/operations/concept_candidates/allowed_labels.txt

USER 1000:1000
ENTRYPOINT ["python", "/app/tools/concepts/local_vlm_worker.py", "--model-dir", "/opt/model", "--socket", "/socket/worker.sock"]
