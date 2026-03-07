# PBI-312: Import, lifecycle, and entity pipeline realignment

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. Import and entity lifecycle behavior is spread across `import.rs`, `import_controller.rs`, `lifecycle_controller.rs`, `metadata_controller.rs`, `sqlite/import.rs`, and parts of `subscription_sync.rs`.
2. Subscription import, manual import, duplicate merge, and collection/entity grouping all feed the same conceptual pipeline but through different code paths.
3. Collection/entity grouping rules from the new entity model are still partially distributed across subscription and duplicate logic.
4. Metadata preservation, status transitions, and post-import refresh semantics are not owned by one domain service.

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
