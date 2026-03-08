# Frontend Settings

Current footprint: `src/features/settings`, about 13 files and about 2.4k lines

## What This Should Own

1. settings panels
2. form state and validation
3. settings save interactions

## What This Should Not Own

1. background runtime orchestration
2. extra feature-level persistence logic

## Why It Is Too Complicated

1. Settings has many panels, which is fine.
2. The risk is that settings becomes the dumping ground for every system integration surface, especially PTR and developer controls.

## Simplification Target

1. settings sections as presentational panels
2. one settings model for save and load

## Concrete Work

1. Keep each panel focused on rendering one subset of settings.
2. Push non-form behavior back to the owning feature or backend.
3. Keep PTR-specific orchestration out of generic settings components.

## Delete Or Merge

1. Delete settings panels that only proxy to one off-host action without real UI value.
2. Merge tiny panels when the split is only cosmetic.

## Test Target

1. settings load, edit, save workflow
2. one PTR settings workflow
3. one duplicates settings workflow
