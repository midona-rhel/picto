# Duplicate Quality Evidence Research

## Executive decision

Picto must stop treating similarity as quality. Duplicate discovery may decide that two files depict
the same content, but that does not identify which file is the better preservation of that content.
For arbitrary images there is no universally correct direction without either a known source or
strong evidence that one candidate is derived from the other.

Smart Merge should therefore use an evidence ladder:

1. Prove decoded equivalence where possible.
2. Apply strict, format-aware dominance rules.
3. Use full-resolution, color-managed difference analysis for directional degradation evidence.
4. Require independent signals and a meaningful confidence margin.
5. Return `NeedsChoice` whenever the evidence is not decisive.

The Difference view should become the human-readable presentation of the same comparison that Smart
Merge uses. It must not remain a separate browser-only visual effect.

## Current implementation audit

### Duplicate discovery

The current pipeline has two stages:

- `core/src/media_processing/phash.rs` reduces luminance to 64×64 and stores a 256-bit low-frequency
  DCT signature plus a 256-bit edge-occupancy mask. This is appropriate for candidate discovery,
  not quality ranking.
- `core/src/duplicates/mod.rs` verifies candidates with 96×96 Lab descriptors. It aligns them by up
  to three pixels and thresholds color differences. It preferentially reads generated thumbnails,
  only rendering from the source when a thumbnail is unavailable.

The value persisted as duplicate `distance` is the spatial verifier's pooled percentage of pixels
whose Delta-E exceeds its threshold. It is not the pHash distance, a full-resolution difference, or
proof of decoded equality. A value of zero only means that no 96×96 descriptor difference survived
the current threshold.

### Smart Merge

`picto-library/src/duplicate.rs` currently ranks candidates with dimensions, MIME type, byte size,
frame count, and the stored low-resolution distance.

The unsafe path is:

- same geometry and frame count;
- spatial distance zero;
- both formats classified as lossless;
- ignore file-size differences;
- choose the lower internal file ID as an automatic tie winner.

That is why a 675 KB PNG can be shown as the winner over a distinct 1,020 KB PNG. The left file is
not judged better; it wins an arbitrary identity tie that the UI presents as quality evidence.

### Difference view

`src/features/duplicates/DuplicatesScreen.tsx` loads both full media URLs and layers them with CSS
`mix-blend-mode: difference`. This is already closer to the evidence a person needs: equal pixels
become black and differences become visible. However, it currently:

- depends on browser decoding and display color management;
- stretches both layers into the same frame;
- has no numeric or spatial summary;
- is not the data used by Smart Merge;
- cannot explain which candidate lost detail, gained artifacts, or changed color.

## What research says

### Full-reference metrics answer a narrower question

SSIM compares a known reference with a distorted image and measures structural degradation. Its
authors explicitly frame it as full-reference image-quality assessment. Picto generally has two
unknown candidates, not an identified original, so SSIM alone can say how different they are but
cannot always establish which is the source. See the
[original SSIM paper](https://www.cns.nyu.edu/~lcv/pubs/makeAbs.php?loc=Wang03).

This is the fundamental limit: two arbitrary images can differ by sharpening, denoising, added
texture, removed noise, a color-grade, or an intentional edit. Without provenance, the pixels alone
cannot always reveal the artist's intent.

### Perceptual maps are valuable evidence

- [Butteraugli](https://github.com/google/butteraugli) models near-threshold visual differences and
  produces both a scalar score and a spatial map. Its own documentation warns that it was tuned for
  a relatively narrow high-quality compression range and is more a research tool than a universal
  format selector.
- [SSIMULACRA 2](https://github.com/libjxl/libjxl/blob/main/tools/ssimulacra2.cc) works across six
  scales in a perceptual color space and separately measures structural difference, ringing, and
  smoothing. Its weights were fitted against multiple human-rated image-quality datasets. The
  ringing and smoothing maps are particularly relevant to directional evidence.
- [FLIP](https://research.nvidia.com/publication/flip) is designed to approximate differences seen
  while alternating two images and returns a perceptual error map. That closely matches the mental
  model of Picto's Difference view.
- [LPIPS](https://openaccess.thecvf.com/content_cvpr_2018/papers/Zhang_The_Unreasonable_Effectiveness_CVPR_2018_paper.pdf)
  correlates learned deep features with human judgments, but it adds model/runtime weight, is less
  explainable, and still does not solve unknown reference direction. It is not a suitable first
  implementation for a desktop library manager.

No one metric should directly choose a winner. Picto needs inspectable evidence and conservative
decision rules.

### Blind quality scoring is the wrong default

No-reference metrics such as BRISQUE and NIQE use statistics learned from natural images. They can
confuse illustration, line art, screenshots, intentional grain, procedural textures, or synthetic
images with degradation. Picto's library is not restricted to natural photographs, so a blind
"cleanliness" score would systematically reward smoothing and punish legitimate texture.

Picto should not rank files by how little noise or high-frequency energy they contain. Noise,
texture, hair, stippling, dithering, and compression artifacts must be distinguished through their
relationship to the other candidate, not counted in isolation.

### File size is evidence, not quality

File size has different meanings by format:

- For two JPEG encodes with the same dimensions and sampling, a larger representation often
  supports a higher-quality hypothesis. JPEG quantization tables are stronger evidence than raw
  bytes, because different encoders have different efficiency. The
  [libjpeg-turbo documentation](https://github.com/libjpeg-turbo/libjpeg-turbo/blob/main/doc/libjpeg.txt)
  exposes those quantization controls.
- PNG filtering and DEFLATE compression are reversible. The
  [PNG specification](https://www.w3.org/TR/png-3/) explicitly defines them as lossless. Two PNGs
  can decode identically while having very different sizes because of filter choice, compression
  effort, chunking, or ancillary metadata. A larger PNG is not automatically sharper.
- If two lossless files decode differently, size still does not identify intent. The difference
  must be analyzed.
- Byte counts are not comparable across codecs. A smaller modern-codec file may preserve more than
  a larger older-codec file.

The correct response to the reported pair is therefore not "left wins". Until full-resolution
analysis is available, it is `NeedsChoice`. The 51% size difference should be displayed as evidence,
but cannot by itself prove that the larger PNG is better.

## Proposed evidence model

Each comparison should return structured evidence rather than only `LeftBetter`, `RightBetter`, or
`NeedsChoice`:

```text
DuplicateQualityEvidence
  relationship
    exact_bytes | exact_decoded | derived_left | derived_right | similar | edited
  decision
    left | right | equivalent | needs_choice
  confidence
    0.0 .. 1.0
  reasons[]
    stable machine-readable code + values + human explanation
  normalized_facts
    dimensions, frames, bit depth, alpha, color profile, orientation
  difference
    changed pixels, mean/p95/max perceptual delta, largest coherent region
  degradation
    blur, ringing, blocking, color shift, clipping, resampling, unexplained residual
  visualizations
    absolute-difference map and perceptual heatmap cache keys
```

Every automatic decision must be reproducible from these fields. The UI should show the primary
reason instead of communicating confidence only through a green border.

## Decision ladder

### Tier 0: byte identity

Equal content hashes are the same physical file. Merge metadata and occurrences without a visual
quality decision.

### Tier 1: decoded equivalence

Decode both candidates at native precision, apply orientation, and transform embedded color
profiles into a shared linear working space. Compare:

- every frame and frame duration;
- dimensions and crop;
- color channels at retained precision;
- alpha independently and composited over black, white, and checker backgrounds;
- HDR transfer characteristics where supported.

If normalized pixels and timing are equal, the candidates are visually equivalent. Prefer the file
that preserves materially richer required data; otherwise prefer the smaller file for efficiency or
keep the user's existing occurrence stable. Do not call either one higher visual quality.

This exact-decoded tier must not silently reduce 16-bit input to 8-bit, discard ICC information, or
flatten alpha before equality is established.

### Tier 2: strict dominance

An automatic winner is safe when one candidate strictly retains capabilities the other lacks and
the shared content agrees:

- higher genuine resolution where downsampling it reproduces the smaller candidate and upscale
  detection does not indicate invented pixels;
- more frames or correct animation timing for the same sequence;
- meaningful alpha where the other candidate has already flattened the same image;
- higher bit depth or wider color encoding with no clipping and matching appearance;
- lossless representation versus a lossy derivative when lineage is strongly established.

Dimensions alone are insufficient when aspect ratio, crop, or composition differs.

### Tier 3: directional degradation

For equal-geometry, non-equivalent candidates:

1. Compute full-resolution normalized difference maps.
2. Evaluate both possible directions: left-as-reference/right-as-derivative and the reverse.
3. Measure smoothing, ringing, block boundaries, clipping, resampling, and coherent edits.
4. Use codec facts such as JPEG quantization and chroma subsampling as supporting evidence.
5. Require at least two independent signals and a calibrated margin before auto-selecting.

SSIMULACRA 2's separate smoothing and ringing maps are the strongest existing design reference for
this stage. Butteraugli or FLIP should be benchmarked as the perceptual-map component, not assumed to
solve direction by themselves.

Raw high-frequency energy must not be a winner signal. A candidate with more energy may contain
detail, intentional grain, sharpening halos, or random noise. The system should measure whether the
residual follows real edges and coherent structures, and fall back to the user when it cannot tell.

### Tier 4: explicit choice

Return `NeedsChoice` when:

- the files are merely perceptually similar;
- the difference is a coherent content edit;
- direction signals disagree;
- only file size or internal age separates them;
- color/HDR/alpha normalization is unsupported;
- the confidence threshold is not met.

There should be no automatic file-ID tie winner for distinct content hashes.

## Difference view as the validation surface

The Difference view should render data produced by the native comparison, not recompute a separate
CSS approximation. A useful comparison has four modes:

1. **Difference:** normalized absolute difference, automatically amplified but with the gain shown.
2. **Perceptual heatmap:** the exact map used by the quality analyzer.
3. **Blink:** alternate normalized candidates at a stable cadence, as supported by FLIP's research
   model.
4. **Wipe:** draggable split view for color, crop, and alignment differences.

Alongside the image, show:

- exact-decoded equality or inequality;
- changed-pixel percentage at zero tolerance and a perceptual tolerance;
- mean, 95th percentile, and maximum perceptual delta;
- largest coherent changed region;
- detected translation/crop;
- bit depth, alpha, profile, subsampling, and frame differences;
- directional artifact findings and the Smart Merge explanation.

ImageMagick's official
[comparison documentation](https://imagemagick.org/compare/) demonstrates the useful combination of
a mathematical metric and a visual difference image. Picto should apply that pattern with consistent
native color normalization and richer perceptual maps.

The comparison should be tiled so large images do not require multiple full-size RGBA copies. Decode
once per candidate, stream normalized tiles through the metrics, and cache only summaries plus
display-sized maps. Review navigation may precompute the next pair at low priority.

## Metric evaluation matrix

| Method | Useful for | Not sufficient for | Project decision |
| --- | --- | --- | --- |
| pHash/detail mask | Fast candidate discovery | Equality or quality direction | Keep discovery-only |
| Exact normalized pixels | Proving equivalence | Ranking different pixels | Required first tier |
| Absolute/RMSE/Delta-E map | Locating and quantifying change | Human visibility and direction | Required diagnostic |
| SSIM/MS-SSIM | Structural similarity | Unknown source direction; local artifacts | Benchmark baseline |
| Butteraugli | Near-JND score and heatmap | Large edits; universal format ranking | Benchmark |
| SSIMULACRA 2 | Multi-scale smoothing/ringing evidence | Unknown source without bidirectional analysis | Preferred directional reference |
| FLIP | Human-readable alternating-image error map | Provenance and arbitrary edits | Preferred UI-map reference |
| LPIPS | Semantic perceptual correlation | Explainability, size, deterministic lightweight shipping | Defer |
| BRISQUE/NIQE | Natural-photo blind quality | Illustration, CG, screenshots, intentional texture | Reject for auto-merge |

## Validation corpus

Build a checked-in manifest whose media fixtures are generated or redistributable. Each pair needs a
human-reviewed relationship, expected evidence, and allowed decision.

### Equivalence

- identical pixels encoded with PNG compression levels at opposite extremes;
- equivalent images with different harmless metadata and chunk layouts;
- equivalent ICC representations normalized to the same appearance;
- equivalent alpha encoded with different compression;
- 8-bit versus 16-bit sources that happen to render the same at 8-bit.

### Known derivations

- JPEG quality and quantization sweeps from one source;
- WebP, AVIF, JPEG XL, and JPEG derivatives at matched perceptual levels;
- downscale and upscale chains using known filters;
- chroma-subsampling variants;
- flattened-alpha derivatives;
- animation frame removal and timing changes.

### Artifact traps

- natural photographs with sensor grain;
- illustrations with flat colors and line art;
- dithering, stippling, halftones, and pixel art;
- denoised versus detailed images;
- sharpened images with halos;
- blocking, ringing, banding, clipping, and color shifts;
- screenshots and text;
- the reported 675 KB/1,020 KB PNG pair.

### Content edits

- watermarks, captions, crops, retouching, and small object changes;
- recolors and exposure changes;
- transparent pixels changed beneath zero alpha;
- pages from the same sequence that pHash groups incorrectly.

## Benchmark protocol

For each metric candidate:

1. Run both candidate orientations where the metric accepts a reference.
2. Record scalar values, maps, wall time, peak memory, and cross-platform determinism.
3. Compare decisions with the corpus labels and human inspection of the Difference view.
4. Report false automatic winners separately from ambiguous fallbacks. A false winner is much more
   costly than `NeedsChoice`.
5. Split results by photographs, illustration, screenshots, alpha, HDR, and animation.
6. Do not tune and validate thresholds on the same pairs.

The release criterion is precision-first: target zero known wrong automatic winners in the
validation set. Coverage may grow gradually after precision is established.

## Implementation sequence

### Phase 0 — safety

- Remove automatic file-ID tie winners for distinct hashes.
- Stop calling a thresholded 96×96 distance of zero exact.
- Return `NeedsChoice` for the reported same-geometry lossless case.
- Add a reason code to every existing decision.

### Phase 1 — native normalized comparison

- Introduce an on-demand comparison service in `core` using source blobs, never thumbnails.
- Reuse the existing ICC-to-sRGB support but retain source precision for equality checks.
- Compute exact equality facts, absolute maps, coherent regions, and display-sized heatmaps.
- Cache results by the ordered content hashes plus an algorithm version in the auxiliary cache,
  avoiding a canonical-library schema migration.

### Phase 2 — one Difference pipeline

- Replace the CSS-only composite with native comparison outputs.
- Add Difference, Heatmap, Blink, and Wipe modes.
- Display quality evidence and the reason an automatic winner is or is not available.

### Phase 3 — metric benchmark

- Benchmark SSIM/MS-SSIM, Butteraugli, SSIMULACRA 2, and FLIP against the corpus.
- Select the smallest cross-platform implementation that meets precision and performance targets.
- Keep metric integration behind an algorithm version so results can be invalidated safely.

### Phase 4 — conservative automation

- Enable strict dominance decisions first.
- Add directional degradation only after corpus validation.
- Treat file size and codec metadata as supporting evidence, never an overriding universal score.
- Preserve `NeedsChoice` as a normal successful outcome.

## Acceptance criteria

1. Distinct hashes can never auto-resolve solely because one file ID is lower.
2. A 96×96 or thumbnail comparison cannot claim decoded equality.
3. Same-pixel PNGs with different compression sizes are recognized as equivalent.
4. Different-pixel PNGs with the same dimensions require evidence beyond file size.
5. Known JPEG quality chains select the higher-fidelity source with a recorded explanation.
6. Intentional noise, dithering, line art, and pixel art do not lose solely for high-frequency
   content.
7. Every Smart Merge result exposes reason codes, measurements, and confidence.
8. The Difference view displays the same normalized comparison data used by Smart Merge.
9. Unsupported and ambiguous cases remain in review without mutating either file.
10. Benchmarks pass on macOS, Windows, and Linux with bounded memory for large images.
