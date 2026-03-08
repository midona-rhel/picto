# Backend Settings

Current footprint: about 4 files and about 383 lines

## What This Should Own

1. persisted settings
2. view preferences storage
3. default values and validation

## What This Should Not Own

1. renderer presentation logic
2. ad hoc migration rules in unrelated domains

## Why It Is Too Complicated

1. This domain is not huge, which is good.
2. The main risk is allowing view preference and application settings policy to spread out again.
3. Settings are easy to over-engineer and should remain stupid persistence plus validation.

## Simplification Target

1. one settings store
2. one view-prefs store
3. clear patch semantics

## Concrete Work

1. Keep defaults and validation in one place.
2. Separate app settings from scoped view preferences clearly.
3. Do not let feature modules invent their own settings persistence.

## Delete Or Merge

1. Delete duplicate settings defaults in the frontend.
2. Merge tiny backend settings helpers if they are purely mechanical.

## Test Target

1. load and save settings workflow
2. get and set view prefs workflow
3. default value migration workflow
