# Frontend Inspector

Current footprint: `src/features/inspector`, about 4 files and about 989 lines

## What This Should Own

1. current selection detail state
2. inspector panel rendering
3. metadata editing interactions

## What This Should Not Own

1. broad data orchestration that really belongs to backend query shapes
2. duplicate mutation logic already present in file or selection actions

## Why It Is Too Complicated

1. Inspector logic is still a common place where fetch orchestration and editing behavior accumulate.
2. The inspector should be a consumer of selection and file data, not a second domain service.

## Simplification Target

1. one inspector data hook
2. one inspector mutation hook
3. presentational sections

## Concrete Work

1. Keep data reading and mutation writing separate.
2. Reuse file and selection action surfaces rather than inventing inspector-specific ones.
3. Keep autosave and undo policies explicit.

## Delete Or Merge

1. Delete inspector-specific mutation wrappers if shared actions already exist.
2. Merge tiny inspector hooks if the split is accidental.

## Test Target

1. open inspector, edit rating or notes, save workflow
2. selection summary update workflow
