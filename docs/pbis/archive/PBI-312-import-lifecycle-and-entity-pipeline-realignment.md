# PBI-312: Import, lifecycle, and entity pipeline realignment

## Priority
P1

## Audit Status (2026-03-15)
Status: **Implemented**

Completion Notes:
1. Import ownership now lives under `./core/src/import/` instead of being spread across legacy flat-root modules.
2. Manual import, folder import, and subscription import share the canonical existing-file merge path in `./core/src/import/existing.rs`.
3. Import pipeline cleanup and duplicate auto-merge follow one lifecycle order:
   - ingest
   - merge/resolve survivor
   - emit downstream ownership and refresh effects from the surviving entity
4. Existing-file lifecycle behavior now consistently handles:
   - status restoration
   - tag merge
   - source URL merge
   - note merge
   - subscription ownership
5. Duplicate merge now preserves folder and subscription ownership on the surviving entity instead of leaving lifecycle state attached to the loser.

## Problem
The backend does not have one clear ingestion and entity lifecycle pipeline. Import, status transitions, metadata merge, duplicate outcomes, and collection/entity grouping rules are spread across several modules. This makes it hard to reason about what happens when new media enters the system or when entities are transformed.

## Scope
- `core/src/import.rs`
- `core/src/import_controller.rs`
- `core/src/lifecycle_controller.rs`
- `core/src/metadata_controller.rs`
- `core/src/sqlite/import.rs`
- relevant collection/duplicate integration paths

## Implementation
1. Define a single entity ingestion/lifecycle service boundary.
2. Separate:
   - raw media ingest
   - entity creation/grouping
   - metadata merge/provenance rules
   - status transitions
   - duplicate/collection integration hooks
3. Make subscription and manual import reuse the same lifecycle primitives.
4. Reduce controller-level orchestration in favor of domain services.

## Acceptance Criteria
1. Import and entity lifecycle ownership is explicit.
2. Subscription/manual import share the same core pipeline.
3. Entity grouping and metadata merge rules are centralized.
4. Status transitions and post-import behavior are easier to test.

## Test Cases
1. Manual import creates expected entity/file state.
2. Subscription import follows the same lifecycle semantics.
3. Duplicate merge and collection grouping still work through the unified pipeline.

## Risk
High. Import and lifecycle behavior touch many user-visible features.
