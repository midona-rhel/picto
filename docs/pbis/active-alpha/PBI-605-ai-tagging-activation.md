# PBI-605: AI tagging activation

## Priority
P1

## Problem

AI tagging has backend model/download/inference code and an uncommitted rebuilt frontend, but
it is not release-proven. The original AI PBI was lost when number 525 was reused, and the
earlier claim of universal GPU acceleration is not true: the current inference session is CPU
only. There are label-parser tests, but no reliable preprocessing, inference, apply, download,
or frontend workflow coverage.

## Locked approach

- Ship a dependable CPU implementation first.
- Treat GPU execution providers as a later measured optimization.
- Keep model files outside the library and verify downloads before activation.
- Predictions are reviewable and are not persisted until the user applies them.
- Applied relations use the canonical AI provenance bit.

## Implementation

1. Finish and review the rebuilt settings and prediction surfaces already present in the
   worktree.
2. Make model download atomic and verify both model and label artifacts before reporting ready.
3. Add deterministic tests for image preprocessing, channel order, output interpretation, and
   namespace thresholds.
4. Add an inference smoke fixture small enough for CI, or a mocked session boundary if the
   production model cannot be distributed in tests.
5. Verify selection prediction, cancellation, review filtering, and apply behavior.
6. Verify AI provenance survives reads, merges, and tag-manager display.
7. Route auto-tag-on-import through background work rather than blocking ingest.
8. Make packaged ONNX Runtime loading explicit on macOS, Windows, and Linux.

## Acceptance criteria

- A model downloads to completion and remains usable after restart.
- Corrupt or incomplete model artifacts are rejected and can be retried.
- Prediction works for one image and a multi-selection without blocking the UI.
- Cancellation ends an active run and leaves no stuck runtime task.
- Applying reviewed tags writes only selected tags with AI provenance.
- Auto-tag-on-import is optional, durable, and does not turn import failure into data loss.
- Settings and prediction UI have focused tests.
- A packaged-app smoke test passes on every release platform using CPU inference.
