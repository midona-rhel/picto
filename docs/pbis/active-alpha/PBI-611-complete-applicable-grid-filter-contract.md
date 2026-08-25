# PBI-611: Complete filter facets and color similarity

## Problem

Picto’s canonical query contract now supports its stored date, duration, byte-size, resolution,
Notes, and URL predicates. Two reference application behavior families still require backend evidence that cannot be
truthfully reconstructed from loaded grid rows:

- date menus show available year-month buckets and counts for every preset/month, while rating,
  media-type, tag, and folder menus show values and counts derived from the entire current scope with
  the other filters applied; Picto's media-type list currently sees only loaded rows plus selections;
- color filtering uses grayscale mode plus palette-ratio-aware CIEDE2000 similarity, while Picto’s
  `file_color` projection stores Lab values but not each palette color’s ratio/marking.

## Contract

- Add one canonical filter-facet query for preset date counts, available `YYYY-MM` buckets, rating,
  media type, tag, and folder values/counts. Each facet must use the current scope and all other
  active predicates while omitting its own predicate, never the loaded page.
- Persist palette ratio/order (and any marking reference application’s matching rule proves necessary) alongside
  each projected Lab color.
- Extend the color filter contract with grayscale mode and accuracy, and compile it with the same
  palette-ratio and CIEDE2000 semantics proven in reference application’s shipped source.
- Reuse the existing filter row and shared floating surface; do not add a client-only fallback.
- Do not add Shape. Do not add Semantic, Fonts, Camera, BPM, or Annotation filters without a
  separately approved data/product contract.

## Verification

- Facet values/counts are exact for presets, month buckets, ratings, types, tags, folders, scope
  changes, and combinations with another active filter.
- Color fixtures prove grayscale, palette-ratio rejection, and accuracy thresholds against reference application
  examples.
- Pages, counts, query-wide selection, exports, and writes resolve the same matching IDs.
- TypeScript, production build, Rust query tests, and contextual interaction tests pass.

Delete this PBI when the acceptance checks pass. Git history is the archive.
