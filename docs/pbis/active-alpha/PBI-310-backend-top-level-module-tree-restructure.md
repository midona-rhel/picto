# PBI-310: Backend top-level module tree restructure

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/lib.rs` exports a long flat list of root modules.
2. `core/src/` still contains dozens of domain/controller files directly at the root.
3. There is no top-level separation between app lifecycle, runtime state, infra, persistence, media processing, and business domains.
4. Contributors currently need filename memory rather than folder structure to navigate the backend.

## Problem
The backend has no clear physical top-level architecture. Even if individual services are improved, the project will remain hard to navigate until `core/src` itself is reorganized into a stable top-level module tree.

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
