# Backend Metadata

Current footprint: about 2 files and about 253 lines

## What This Should Own

1. file metadata reads
2. file metadata updates
3. metadata aggregation for detail views

## What This Should Not Own

1. media extraction pipeline implementation
2. selection UI behavior

## Why It Is Too Complicated

1. The domain itself is not large.
2. The problem is that metadata behavior overlaps with file lifecycle and media processing.
3. If this boundary stays fuzzy, both backend and frontend compensate with extra layers.

## Simplification Target

1. metadata query service
2. metadata mutation service
3. media-processing handoff boundary

## Concrete Work

1. Keep metadata aggregation here, not in renderer helpers.
2. Keep extraction and analysis in media-processing, not in metadata.
3. Make detail-view payloads explicit.

## Delete Or Merge

1. Delete frontend normalization that exists only because backend metadata shape is inconsistent.
2. Merge tiny metadata wrappers into the real service surface.

## Test Target

1. get file all metadata workflow
2. set notes workflow
3. set source URLs workflow
