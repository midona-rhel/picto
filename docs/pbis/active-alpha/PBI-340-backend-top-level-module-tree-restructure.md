# PBI-340: Backend top-level module tree restructure

## Priority
P1

## Audit Status (2026-03-08)
Status: **Partially Implemented**

Evidence:
1. `core/src/lib.rs` now exposes real domain folders like `duplicates`, `folders`, `grid`, `metadata`, `selection`, `smart_folders`, `subscriptions`, and `tags`.
2. The root is no longer a flat bag of controller files, but the target top-level architecture is still incomplete.
3. There is still no explicit `app/`, `runtime/`, `infra/`, or `persistence/` split; cross-cutting modules remain mixed at root.
4. Top-level navigation is better than before, but the intended stable module tree is not complete yet.

## Problem
The backend has no clear physical top-level architecture. Even if individual services are improved, the project will remain hard to navigate until `core/src` itself is reorganized into a stable top-level module tree.

Reference architecture: `docs/rust-core-rearchitecture-blueprint-2026-03-07.md`

## Scope
- `core/src/lib.rs`
- `core/src/` top-level structure
- target architecture in `docs/rust-core-target-module-tree-2026-03-07.md`

## Implementation
1. Introduce top-level folders:
   - `app/`
   - `runtime/`
   - `infra/`
   - `media_processing/`
   - `persistence/`
   - `domains/`
   - keep `dispatch/`
2. Update `lib.rs` to reflect the new top-level tree.
3. Move root-level modules into the correct top-level bucket in staged batches.
4. Leave temporary re-export aliases only where needed during migration.

## Acceptance Criteria
1. `core/src` root is no longer a flat bag of domain files.
2. Top-level responsibilities are physically separated.
3. `lib.rs` reflects the new architecture clearly.
4. The module tree matches `docs/rust-core-target-module-tree-2026-03-07.md`.

## Test Cases
1. `cargo build`
2. `cargo test`
3. `native/picto-node` build still resolves imports.

## Risk
Medium-high. Broad mechanical refactor with many import path updates.
