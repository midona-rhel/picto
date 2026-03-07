# PBI-312: Backend persistence layer split between shared and domain-owned DB modules

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/sqlite/` still contains both shared DB infrastructure and domain-specific persistence logic.
2. `core/src/sqlite_ptr/` similarly mixes shared PTR storage infrastructure and domain-local logic.
3. This keeps persistence ownership separate from the domains that actually use it.

## Problem
A contributor should not have to jump between `domains/*` and monolithic `sqlite/*` folders just to understand one domain. Shared DB infrastructure should stay centralized, but domain-specific queries and write helpers should move under domain ownership.

## Scope
- `core/src/sqlite/*`
- `core/src/sqlite_ptr/*`
- new domain-local `db.rs` modules
- new shared `persistence/` folder

## Implementation
1. Create `core/src/persistence/` for shared DB infrastructure only.
2. Move domain-specific query/write modules into their owning domain folders as `db.rs` or submodules.
3. Leave only shared infrastructure in `persistence/`:
   - connection pool
   - schema runner
   - migration registry
   - shared publish/manifest helpers
4. Reduce cross-folder ownership ambiguity.

## Acceptance Criteria
1. Domain-local persistence is physically close to domain logic.
2. Shared persistence infrastructure is clearly separate.
3. `sqlite/` and `sqlite_ptr/` no longer act as giant second homes for domain behavior.

## Test Cases
1. Build/tests pass.
2. Representative domain CRUD/query flows still work.

## Risk
High. Moves correctness-sensitive SQL helpers and must be staged carefully.
