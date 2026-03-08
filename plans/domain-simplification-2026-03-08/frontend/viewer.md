# Frontend Viewer

Current footprint: `src/features/viewer`, about 15 files and about 2.3k lines

## What This Should Own

1. detail and slideshow presentation
2. media playback UI state
3. navigation within the current image set

## What This Should Not Own

1. grid query logic
2. backend metadata policy

## Why It Is Too Complicated

1. Viewer behavior overlaps with grid detail behavior and separate detail windows.
2. The viewer should be a consumer of grid context, not a second grid system.

## Simplification Target

1. one viewer session model
2. one slideshow controller
3. one detail presentation layer

## Concrete Work

1. Keep current-item navigation and playback state local to viewer code.
2. Treat grid as the owner of image-set context.
3. Keep detail-window behavior thin and dependent on the same viewer model.

## Delete Or Merge

1. Delete duplicated session logic between grid detail and viewer if both exist.
2. Merge detail presentation helpers when they are only split by window context.

## Test Target

1. open item, navigate next and previous, close workflow
2. slideshow workflow
3. media playback fallback workflow
