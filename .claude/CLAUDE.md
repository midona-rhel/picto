# Picto Project Instructions (Current)

## Runtime Architecture

Picto is an Electron desktop app with:

1. Rust core in `core/src/` (SQLite + Roaring bitmaps + orchestration)
2. Electron host in `electron/` (windows, IPC bridge, media protocol)
3. Native Node addon bridge in `native/picto-node/` (N-API bindings into Rust core)
4. React frontend in `src/`

## Key Paths

### Backend Core (`core/src/`)

1. `sqlite/`:
   - primary DB layer, schema, CRUD, compilers, projections, sidebar reads
2. `sqlite_ptr/`:
   - PTR database and sync/bootstrap logic
3. `dispatch/`:
   - command routing by domain (`files_*`, `folders`, `tags`, `subscriptions`, `system`, etc.)
4. `events.rs`:
   - event emission contracts (`state-changed`, `sidebar-invalidated`, grid invalidation events)
5. `subscription_sync.rs` / `subscription_controller.rs`:
   - gallery-dl ingestion and flow execution
6. `grid_controller.rs`:
   - grid page queries and pagination surfaces
7. `state.rs`:
   - library lifecycle (`open_library`, `close_library`) and worker ownership

### Desktop Host (`electron/`)

1. `main.mjs`:
   - process bootstrap, window creation, library switching, IPC handlers
2. `preload.cjs`:
   - secure renderer API surface
3. `nativeClient.mjs`:
   - wrapper around the N-API addon (`initialize`, `openLibrary`, `invoke`, events)
4. `globalConfig.mjs`:
   - library history/pinning metadata

### Frontend (`src/`)

1. `stores/domainStore.ts`:
   - sidebar counts/tree + smart-folder summaries
2. `stores/cacheStore.ts`:
   - grid cache/refresh state
3. `stores/eventBridge.ts`:
   - native event ingestion and invalidation fanout
4. `components/image-grid/`:
   - grid UI, selection, query broker, live insert ordering

## Data Model Conventions

1. Status model:
   - `0=inbox`, `1=active`, `2=trash`
2. Internal identity:
   - dense integer IDs in DB, SHA256 hash at API boundary
3. Tags:
   - `(namespace, subtag)` normalized storage
4. Blob store:
   - content-addressed file and thumbnail storage under library root

## CI / Gates

Primary scripts (`package.json`):

1. `gate:alpha` (blocking release lane)
2. `alpha:verify` (TS + frontend tests + core tests + command parity)
3. `alpha:package` (electron-builder package)
4. `alpha:smoke` (scripted smoke scenarios)
5. `gate:legacy` (non-blocking debt/guard lane)

Workflows:

1. `.github/workflows/alpha-gate.yml`
2. `.github/workflows/legacy-gate.yml`

## Documentation Policy

Documentation is intentionally minimized:

1. Keep docs under `docs/pbis/` (active + archive)
2. Treat `docs/pbis/active-alpha/` as the current alpha backlog source of truth
3. Avoid adding broad strategy/audit documents outside `docs/pbis/` unless explicitly requested
