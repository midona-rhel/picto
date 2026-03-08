# Frontend Platform Bridge

Current footprint: `src/platform`, about 6 files and about 827 lines

## What This Should Own

1. typed command calls to the backend
2. host-only window and native integrations
3. event listen and emit plumbing

## What This Should Not Own

1. feature-specific normalization scattered across the app
2. extra pass-through wrapper layers on top of it

## Why It Is Too Complicated

1. `src/platform/api.ts` is still a god-client.
2. It mixes backend commands, Electron host operations, response normalization, and type escapes.
3. The project improved by centralizing transport, then immediately started re-growing complexity on that surface.

## Simplification Target

1. one `coreApi`
2. one `hostApi`
3. one small normalization layer

## Concrete Work

1. Split backend command calls from host-only Electron operations.
2. Move response normalization to dedicated adapters.
3. Remove type escapes where generated types can carry the load.

## Delete Or Merge

1. Delete thin controller wrappers that just call this API.
2. Merge one-off normalizers into the bridge boundary instead of feature components.

## Test Target

1. command contract tests
2. runtime event listener contract tests
3. host API smoke tests
