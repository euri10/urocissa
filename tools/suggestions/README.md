# Suggested tags

This owner-only tier is separate from Auto tags. It uses the frozen
`onnx-community/Florence-2-large-ft` INT8 revision
`04ace74913c28d7ec94af32cee5c111a10126b6f`, Transformers.js 4.3.2, and
OD, dense-region and more-detailed-caption tasks with three beams and 256
maximum new tokens. `model.sha256` pins all model and processor files.

The mapping copies the exact 2026-11-25 public screen: reviewed original and
expanded noun aliases, the frozen parent graph, exclusions for flower, rose,
sunflower and ground, neutral person aliases, phrase suppression and an
eight-label cap. The source inputs had SHA-256 `4033b36d89afcb49630d8b039b896176ed2e4fe6a2b5b180a21524127d39113a`
and `99aea94db03a3de093cd1cbfd3a5c0562bda36a3cb02c9d0f22eacd99dd85ffb`;
the original public mapper was `9dab0101f2c87f1881f223c8f9ff80eed69a0b5501dac9321ba76370542cf432`.
The adapted mapping reproduced all 64 frozen public predictions exactly.
Those public results were about 3 labels/photo and 76.4% conservative
agent-judged precision; they did not pass the 90% Auto-tag gate.

The worker accepts bounded JPEG bytes over a private Unix socket. Node sends
raw generation results only through an anonymous pipe to the Python mapper;
only mapped labels reach the gallery. The Python process discards stdout and
stderr, sends Node's stderr to `/dev/null`, disables core dumps and sends fixed
errors only. Console output is disabled in Node. This explicitly contains the
Transformers.js tensor-logging failure path. Deployment must also use a
networkless, read-only container with logging disabled and no gallery mount.

The gallery stores disposable results in `suggestions.redb`, keyed by photo,
thumbnail version and this frozen model/mapping version. Existing User tags
and durable Rejections suppress matching suggestions when read or searched.
Confirm and Reject reuse the ordinary Concept feedback path; neither changes
the frozen Auto-tag review. Pending work is reconstructed after restart, and
new uploads take priority over the remaining backfill. Turning Auto tags off
for a photo also prevents suggestions for that photo.
Gallery shutdown interrupts an in-flight socket request and joins its cache
writer before the stopped-volume backup; the worker does not mount the data.

Run `python3 -m unittest discover -s tools/suggestions -p 'test_*.py'` for
the mapping and logging checks. End-to-end worker, owner/visitor and
old-gallery rollback checks require disposable fixtures; production photos
and raw model text must never leave the owner's hosts.
