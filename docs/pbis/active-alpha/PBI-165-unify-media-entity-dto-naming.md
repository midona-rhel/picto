# PBI-165: Unify DTO naming from file/image-centric to entity-centric contracts

## Priority
P0

## Audit Status (2026-03-03)
Status: **Not Implemented**

## Problem
API and core DTO names are mixed (`ImageItem`, `FileInfo`, `FileInfoSlim`) while architecture is moving to `media_entity` + collections. Naming mismatch increases implementation mistakes and slows onboarding.

## Scope
- `/Users/midona/Code/imaginator/core/src/types.rs`
- `/Users/midona/Code/imaginator/src/types/api.ts`
- `/Users/midona/Code/imaginator/src/controllers/*`
- `/Users/midona/Code/imaginator/src/components/image-grid/*`

## Implementation
1. Introduce canonical DTO names:
   - `EntitySlim`
   - `EntityDetails`
   - `EntityMetadataBatchResponse`
2. Keep temporary type aliases for migration window.
3. Update controller/component call sites to canonical names.
4. Add CI guard to block new file-centric DTO names for user-facing entity surfaces.

## Acceptance Criteria
1. Entity-facing APIs and UI use entity terminology consistently.
2. Collection support can be added without semantic confusion in contracts.
3. No new user-facing DTOs use ambiguous file/image naming.

## Test Cases
1. Typecheck passes after DTO rename migration.
2. Grid/detail/properties render unchanged for existing single entities.

## Risk
Medium. Broad TS/Rust contract touch points; stage with aliases.

