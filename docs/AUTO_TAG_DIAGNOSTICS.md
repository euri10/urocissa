# Owner-only Auto tag diagnosis

`GET /get/auto-tags/PHOTO_HASH/explanation?concept=bridge` reads one current
cached photo embedding and the existing fitted engine. It performs no model
inference, fitting, recalculation, feedback or database write. It uses the same
owner authentication as the existing per-photo Auto tags endpoint; anonymous,
public and share credentials are refused before photo lookup.

Use this inside the authenticated application when investigating a reviewed
photo. Keep personal scores and outputs in the application; do not export them
or save responses in terminal logs, screenshots or diagnostic files. Aggregate
decision counts may be recorded in Beads. Public/synthetic fixtures may be used
for reproducibility outside the application.

The response names the actual cached learning mode and whether learning or
materialization is pending. `decision` follows the engine's precedence: `off`,
`rejected`, `authored`, then `displayed`, `overCap`, `suppressedAncestor` or
`implied`, otherwise `inactive` or `belowThreshold`. `viaUserTags` identifies
authored descendants implying a parent, and `viaAutoTags` identifies predicted
descendants. A parent can therefore appear in review even when its own scoring
is inactive. This distinction matters for building/house false positives.

`zeroShotScore` is the visual prompt score; `matchScore` and `threshold` describe
the actual classifier decision, or are null for an inactive Concept. A bridge
with fewer than five positive or five negative examples retains its calibrated
built-in decision. A missed chip alone cannot identify a scoring defect. These
scores are diagnostic values, not a calibrated probability of correctness on
the owner's library, and no score can replace the owner's visual judgement.

Unknown Concepts, missing photos and unavailable current embeddings return 404;
an unprepared Auto tag service returns 503. Pending results must be rechecked
after normal learning completes. The endpoint does not alter authored V6
records or derived formats and needs no data migration. Ordinary image/config
rollback retains the existing volume and feedback.

## Teach a missing Concept a genuine negative

An empty Concept review need not prevent explicit teaching. In the authenticated
application at <https://photos.sslc.rocks>, open a photo you know does **not**
contain the Concept, then **Info -> Build concepts**. In **Add your own tag**,
enter `bridge` and click **Not in this photo**. No generated suggestion is needed.
The saved row reads `bridge · rejected`; **Undo rejection** reverses this decision.
This records a Rejection, not a User tag. **Add tag** and Enter remain positive
confirmation actions. Existing authored positives cannot be silently overwritten;
after an uncertain/conflicting response refresh before trying again.

For the consolidated owner round, use only already-selected teaching photos that
genuinely lack a bridge, and only after the initial frozen review has been saved.
Never teach from the garden held-outs or the random precision sample. Five distinct
genuine negatives plus the existing positives permit the unchanged learned gate;
they do not guarantee precision or justify weakening quality acceptance. Do not
manufacture negative examples merely to reach five. Keep the entire first sitting
within the single acceptance lane's 20-photo limit.
