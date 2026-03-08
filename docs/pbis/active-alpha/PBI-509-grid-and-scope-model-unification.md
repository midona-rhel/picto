# PBI-509: Grid and scope model unification

## Priority
P0

## Problem
Grid behavior is split across too many scope rules, refresh paths, and frontend-owned decisions.

## Goal
Replace ad hoc grid orchestration with one backend query model.

## Public Query Shape
1. `scope_kind`
2. `scope_id` or predicate
3. lifecycle filter
4. tag filters
5. folder filters
6. color filters
7. sort
8. cursor

## Implementation
1. One backend grid query service handles all scopes.
2. Non-collection scopes exclude grouped collection members.
3. Grid fallback polling remains temporary migration-only behavior.
4. Frontend grid becomes a renderer over backend scopes and read models.

## Acceptance Criteria
1. No parallel grid semantics exist by feature.
2. Scope behavior is centralized and documented.
