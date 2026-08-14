# PBI-605: AI tagging activation

## Priority

P1 release blocker

## Behavior

Picto can download a registered CPU tagger model, predict tags for selected images, let the user
review those predictions, and apply only the accepted tags with AI provenance. Optional automatic
tagging runs after import as durable background work; it never delays or invalidates the import.

A model is available only when its complete model-and-label bundle has been downloaded, validated,
and activated. Cancellation or failure leaves no active model, partial bundle, stale result, or stuck
runtime task.

## Proven unfinished behavior

- Z3D is currently fed RGB although its reference contract requires BGR.
- Unknown model names can reach filesystem deletion paths.
- Downloads write into the active directory, report success before completion, cannot be cancelled,
  and can leave mixed model/label artifacts.
- File existence is treated as model readiness without validating labels or ONNX inputs/outputs.
- Reviewed and automatic tagging use separate threshold/apply paths.
- Automatic tagging blocks ingest and disappears on failure or restart.
- Prediction cancellation is reported as successful completion and stale runs can update a reopened
  panel.
- Query-result selection exposes Auto Tag even though the command only accepts explicit hashes.
- Reviewed apply closes before all writes succeed and can leave an unexplained partial result.
- No deterministic inference fixture or packaged CPU inference smoke proves the shipped behavior.

## Tickets

1. **Registered model bundles**
   Resolve every model operation through the static registry. Fix model-specific preprocessing,
   reject unknown slugs, and never construct a model path from unchecked input.

2. **Atomic model activation**
   Download model and labels into one temporary directory, support cancellation, validate labels and
   ONNX input/output contracts, then rename the complete bundle into place. Delete temporary data on
   every failure or cancellation and report ready only after validation.

3. **One prediction executor**
   Put decode, preprocessing, inference, output interpretation, namespace mapping, and thresholding
   behind one tested per-entity helper. Reviewed prediction and automatic tagging call that helper.

4. **Durable automatic tagging**
   Enqueue one idempotent `ai_tag` background job per imported media entity. The worker retries
   failures, survives restart, applies accepted tags with AI provenance, and emits normal tag facts.

5. **Truthful review lifecycle**
   Give each reviewed run explicit ownership and cancellation. Disable unsupported query-result
   targets. Await one authoritative batch apply result and do not close on failure. Settings and the
   review surface use the same threshold contract.

6. **Release proof**
   Test preprocessing, channel order, output shape and interpretation, bundle failure/cancellation,
   queue restart/retry, reviewed apply, AI provenance, and stale-run rejection. Run a real CPU
   inference smoke from packaged macOS, Windows, and Linux builds.

## Acceptance

- A valid registered model remains ready after restart; unknown, corrupt, incomplete, or cancelled
  bundles are absent and retryable.
- One image and an explicit multi-selection produce deterministic review results without blocking the
  UI.
- Cancelling or closing a run cannot update a later run and leaves no runtime task behind.
- Apply writes only accepted tags with AI provenance and reports success or failure before closing.
- Auto-tag-on-import is optional, durable, retryable, and does not block import completion.
- Focused backend/frontend tests and packaged CPU inference smokes pass on all release platforms.

## Out of scope

- GPU execution providers. Add them only after the CPU path is release-proven and measured.
- A broad AI-panel visual redesign. Reuse Picto controls now; visual exploration belongs to the later
  reference application-reference UI pass.
