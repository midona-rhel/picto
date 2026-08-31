# PBI-620: Evidence-Based Duplicate Quality

## Observed Gap

Smart Merge can mark one distinct lossless file as the quality winner when the only decisive fact is
its lower internal file ID. A zero stored duplicate distance currently means that a thresholded
96×96 comparison found no significant difference; it does not prove full-resolution equality.

The full-image Difference view exposes changes to the user, but Smart Merge does not use the same
pixels or difference evidence.

## Required Behavior

- Distinct hashes never auto-resolve through a stable file-ID tie.
- Similarity, decoded equivalence, and quality direction are separate states.
- Smart Merge uses source-blob, color-normalized evidence and explains every decision.
- File size is format-aware supporting evidence, not a universal quality score.
- Intentional noise or high-frequency detail is not treated as degradation in isolation.
- Difference view and Smart Merge consume the same native comparison result.
- Ambiguous cases return `NeedsChoice` without mutation.

The research basis, decision ladder, corpus, and staged implementation are defined in
[`../../DUPLICATE_QUALITY_RESEARCH.md`](../../DUPLICATE_QUALITY_RESEARCH.md).

## Acceptance

1. The reported same-resolution 675 KB and 1,020 KB PNG pair cannot select the left file by ID.
2. Same-pixel PNGs with different compression sizes are classified as equivalent.
3. Different-pixel lossless files do not select a winner from byte size alone.
4. Known lossy derivation chains select the higher-fidelity source with measurements and reasons.
5. Illustration, photographs, screenshots, alpha, animation, dithering, grain, blur, ringing,
   blocking, crop, and color-profile fixtures cover both safe winners and required choices.
6. Difference, Heatmap, Blink, and Wipe render from the same normalized comparison used by the
   backend decision.
7. All automatic decisions are deterministic across supported platforms and algorithm-versioned.
