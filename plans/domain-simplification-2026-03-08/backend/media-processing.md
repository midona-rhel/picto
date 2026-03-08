# Backend Media Processing

Current footprint: about 11 files and about 3.6k lines

## What This Should Own

1. media inspection
2. thumbnails and blurhash
3. format-specific adapters
4. ffmpeg and related binary path resolution

## What This Should Not Own

1. import orchestration
2. metadata domain decisions
3. gallery-dl subscription orchestration

## Why It Is Too Complicated

1. Media processing has grown into a platform without a simple adapter model.
2. Format handling is still closer to "helpers around a large module" than a clean capability registry.
3. This is the kind of domain that quietly accumulates half the project if it is not policed.

## Simplification Target

1. one media inspection pipeline
2. one adapter registry
3. format adapters that are easy to add or remove

## Concrete Work

1. Split detect or inspect from format-specific handlers.
2. Make adapters explicit for image, video, archive, office, pdf, svg, and specialty cases.
3. Keep binary path resolution separate from processing logic.

## Delete Or Merge

1. Delete generic helper piles once adapters exist.
2. Merge tiny format handlers only when they truly share one implementation path.

## Test Target

1. image inspection workflow
2. video thumbnail workflow
3. archive or office handling workflow
