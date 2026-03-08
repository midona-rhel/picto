# PBI-343: Backend controller elimination and service boundary normalization

## Priority
P1

## Audit Status (2026-03-08)
Status: **Partially Implemented**

Evidence:
1. Controller naming is no longer a flat-root problem, but many domain folders still use `controller.rs` for modules that are really orchestration or service boundaries.
2. Examples include `core/src/subscriptions/controller.rs`, `core/src/metadata/controller.rs`, `core/src/duplicates/controller.rs`, and `core/src/smart_folders/controller.rs`.
3. The term `controller` still obscures whether a file is a thin domain entry point, a query service, or an orchestrator.

## Problem
Even after moving files into folders, the backend will remain confusing if service boundaries are still expressed as vague `controller` modules. Physical structure and naming need to align.

Reference architecture: `docs/rust-core-rearchitecture-blueprint-2026-03-07.md`

## Scope
- domain modules under `core/src/domains/*`
- app/runtime modules where controller naming remains misleading

## Implementation
1. Audit every `*_controller.rs` module after folderization.
2. Rename modules by actual responsibility:
   - `controller.rs` only if it is truly the thin public domain entry point
   - `orchestrator.rs` for run/process coordination
   - `query.rs` for read services
   - `lifecycle.rs` for status/process transitions
   - `service.rs` where appropriate
3. Delete naming that encodes no meaningful boundary.

## Acceptance Criteria
1. Module names reflect actual responsibility.
2. `controller` is no longer used as a catch-all for everything.
3. Domain folders become easier to reason about by file names alone.

## Test Cases
1. Build/tests pass after renames.
2. New contributors can infer module role from name without spelunking implementation.

## Risk
Medium. Mostly structural/naming work, but broad import churn.
