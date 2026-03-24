# PBI-580: Cross-layer comment rules

## Priority
P1

## AI-generated caveat
This document is intentionally strict. The goal is to stop comments from becoming a second stale design document inside the code.

## Lifecycle
- `Implemented` when the comment rules are written, linked, and specific enough to apply.
- `Activatable` when the active reset PBIs explicitly adopt these rules.
- `Activated` when migrated reset slices actually follow the comment discipline.
- `Legacy removed` when migrated reset slices no longer keep the replaced stale comment style.

## Problem
The project currently has too many comments that:

- describe old architecture instead of the current code
- explain the history of a refactor instead of the steady-state rule
- mention a PBI, migration step, or temporary implementation moment that will not matter later
- narrate obvious code instead of clarifying a real invariant or tricky behavior

That makes comments noisy, stale, and harder to trust.

This PBI locks one comment discipline for the greenfield reset set so the database, backend, media delivery, frontend boundary, and UI reset all leave behind comments that describe the real code being shipped.

This is a prerequisite PBI for the greenfield reset set. Read and apply it before implementing the other reset PBIs in this series.

## Comment goals
Comments should make these things true:

- comments describe the code that exists now
- comments explain invariants, boundaries, non-obvious behavior, or product rules
- comments do not depend on the reader knowing a PBI number or refactor story
- comments age well after the implementation is complete

## Rules

### 1. Comments must describe current code, not implementation history
Good:
- why a collection uses `primary_member_entity_id`
- why grouped queries count a collection once
- why a controller applies a direct local update before later reconciliation

Bad:
- “implemented for PBI-567”
- “temporary until old sqlite path is removed”
- “new path after refactor”

If the comment will become false or useless once the PBI lands, do not write it.

### 2. Comments must not narrate obvious code
Do not add comments that just restate what the next line already says.

Comments are for:
- invariants
- cross-boundary rules
- tricky query behavior
- fallback/recovery rules
- non-obvious performance or correctness constraints

### 3. Do not leave migration-story comments in steady-state code
Migration and cutover notes belong in:
- PBIs
- migration code
- commit messages

They do not belong in normal write/query/projection/controller/component code once the new path is the real path.

### 4. Boundary comments are allowed and encouraged
Good boundary comments explain:
- why a module exists
- what it is allowed to read or write
- what must stay internal
- which side owns a behavior

Examples:
- `db/write` owns writes to authoritative tables only
- media delivery hides file paths from the frontend
- controllers own direct visible updates while backend state changes reconcile the rest

### 5. Tests follow the same discipline
Test comments should explain:
- the behavior family being proven
- the edge case being locked

They should not explain:
- that this was added because a PBI asked for it
- that this replaced an older test shape

### 6. Delete stale comments during the work
Each reset PBI should explicitly remove comments that:
- describe deleted architecture
- mention old DTO names or old boundaries
- explain old transport/storage structure
- narrate implementation history instead of current behavior

## Required standard for the reset line
Every reset PBI in the `567+` line must follow these rules:

- no comment should mention “this PBI”, “this refactor”, or similar implementation-story wording in production code
- comments should describe invariants, boundaries, or non-obvious behavior only
- stale comments in the touched area must be removed as part of the work

## Done definition
This PBI is satisfied for a given reset PBI only when:

- that PBI references this comment-rules document
- its touched code does not introduce implementation-story comments
- stale comments in the touched slice are removed or rewritten
- new comments explain the actual shipped behavior, not the transition to it

## Manual review checklist
- Does each new comment describe the current code or rule?
- Would the comment still make sense six months later after the PBI is long merged?
- Does the comment explain something non-obvious?
- Could the comment be removed because the code already says the same thing?
- Did the implementation remove stale comments in the same slice?
