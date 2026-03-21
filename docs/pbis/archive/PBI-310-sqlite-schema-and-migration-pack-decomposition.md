# PBI-310: SQLite schema and migration pack decomposition

## Priority
P1

## Audit Status (2026-03-07)
Status: **Implemented**

Completion Notes:
1. `core/src/sqlite/schema.rs` is now a thin facade over explicit schema packs under `core/src/sqlite/schema/`.
2. DDL, migration history, schema reconciliation, shared helpers/seeding, and tests now live in separate files:
   - `ddl.rs`
   - `migrations.rs`
   - `reconcile.rs`
   - `support.rs`
   - `tests.rs`
3. Migration ordering remains centralized in one runner while domain-owned reconciliation logic is separated from fresh-init DDL.
4. Targeted schema tests cover fresh init, full migration replay, and reconciliation of missing current tables.

## Problem
The SQLite schema layer has become a monolithic migration file. It is still functional, but it is not maintainable as the database continues to evolve. Domain ownership is obscured and future schema work will keep getting riskier.

## Scope
- `core/src/sqlite/schema.rs`
- migration/version helpers in `core/src/sqlite/mod.rs` as needed

## Implementation
1. Split schema initialization and migration definitions into domain-oriented packs.
2. Keep one central migration runner, but make domain packs explicit:
   - core tables
   - entities/files
   - folders/smart folders/sidebar
   - tags
   - subscriptions/flows/credentials
   - duplicates
   - projections/read models
3. Preserve strict migration ordering and historical safety.
4. Add a documented migration manifest so reviewers can see which domain owns each step.

## Acceptance Criteria
1. `schema.rs` is decomposed into domain migration packs.
2. Migration ordering remains deterministic.
3. Domain ownership of schema changes is obvious.
4. Reviewers can modify one migration area without spelunking unrelated history.

## Test Cases
1. Fresh database init still works.
2. Upgrade from older schema versions still works.
3. Existing migration tests or smoke checks continue to pass.

## Risk
Medium. Mostly mechanical, but migration ordering mistakes would be serious.
