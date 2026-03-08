# Frontend Sidebar

Current footprint: `src/features/sidebar`, about 17 files and about 2.7k lines

## What This Should Own

1. sidebar rendering
2. tree interactions
3. folder and smart-folder navigation
4. library-switcher UI

## What This Should Not Own

1. folder business rules
2. backend refresh policy

## Why It Is Too Complicated

1. Folder tree behavior, menu policy, DnD, rename flows, and navigation still sit close together.
2. Sidebar is partly a feature and partly a home for folder-related UI debt.

## Simplification Target

1. tree view model
2. navigation actions
3. context menu policy

## Concrete Work

1. Keep folder tree DnD in one model.
2. Keep menu policy centralized.
3. Push actual folder operations down to the folder domain or backend API.

## Delete Or Merge

1. Delete duplicated action-matrix logic.
2. Merge tree helper code if it is split only for historical reasons.

## Test Target

1. folder drag-drop workflow
2. rename and delete workflow
3. smart-folder navigation workflow
