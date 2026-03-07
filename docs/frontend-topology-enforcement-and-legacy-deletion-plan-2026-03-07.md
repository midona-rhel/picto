# Frontend Topology Enforcement and Legacy Deletion Plan

Date: 2026-03-07
Related structural umbrella: `PBI-401`

## Purpose

The frontend does not just need a target structure. It needs policy, tooling,
and a deletion program so the structure stays good and legacy actually gets
removed.

Without that, migration work will degrade into:

1. file shuffling without ownership cleanup
2. compatibility layers that never get deleted
3. new work landing in old folders because nothing blocks it

## Structural Rule

After the topology migration starts, `src/` root should become effectively
read-only.

Allowed top-level entries:

1. `app/`
2. `entrypoints/`
3. `features/`
4. `shared/`
5. `state/`
6. `platform/`
7. `test/`
8. `vite-env.d.ts`
9. temporary top-level style/module files required by the bundler during migration only

Anything else should be treated as legacy or migration residue.

## Canonical Ownership Rules

### `src/features/*`

Owns:

1. domain-specific UI
2. domain-local hooks/controllers/helpers
3. domain-specific state that should not be app-global

Must not own:

1. cross-domain primitives
2. Electron bridge code
3. random generic helpers

### `src/shared/*`

Owns only code that is genuinely reused across domains:

1. shared presentational components
2. shared hooks
3. shared utilities
4. shared styles
5. shared types
6. shared services/portals that are not domain-owned

If something is used by only one feature, it is not shared.

### `src/state/*`

Owns app-global stores only.

If state is owned by one feature, it belongs in that feature, not in app-global
state.

### `src/platform/*`

Owns Electron/desktop adapters and transport bindings only.

No domain logic should hide in platform wrappers.

### `src/app/*`

Owns application shell composition, bootstrap wiring, and composition-root
concerns.

### `src/entrypoints/*`

Owns the runtime entry files only:

1. main window
2. detail window
3. settings window
4. subscriptions window
5. library manager window

## CI Guardrails

Add hard checks for:

1. no new top-level frontend files/folders outside the approved root list
2. no new imports from deprecated legacy paths
3. no new feature/domain code landing under legacy `src/components/*` catch-all paths
4. no new compatibility shims without explicit legacy markers
5. file size ceilings for hotspot files
6. no new direct imports that bypass a feature's public surface once that feature is migrated

### Practical examples

Fail CI if:

1. a new file appears directly under `src/` and it is not an approved entrypoint/build file
2. a new feature PR adds imports from legacy domain-owned files under `src/components/`
3. a file marked `LEGACY:` is still imported after its replacement has landed
4. hotspot files exceed agreed line-count ceilings without an explicit override

## Legacy Register

Create and maintain:

- `docs/frontend-legacy-register.md`

Every legacy item must include:

1. path
2. category
3. reason it still exists
4. replacement path or target owner
5. delete condition
6. owning PBI
7. status:
   - `transitional`
   - `delete-now`
   - `merge-then-delete`
   - `blocked`

If a temporary path is not in the register, it is not temporary. It is just
uncatalogued rot.

## PR Policy

Every frontend restructuring PR must answer:

1. what folder/domain owns this now?
2. what old path becomes obsolete because of this?
3. what code got deleted?
4. what legacy marker was removed?

No topology PR should only add structure. It must either:

1. move code toward canonical ownership
2. or delete obsolete paths

## Delete Budget

Set an explicit deletion policy for the cleanup campaign:

1. every frontend restructuring PR should delete code
2. target deletion budget per cleanup PR:
   - `300-1500 LOC`
3. no move-only PRs unless unavoidable for a migration seam

This keeps the program from becoming endless reorganization with no legacy
removal.

## Classification Pass

Every frontend file should get one label:

1. `canonical`
2. `transitional`
3. `legacy-delete`
4. `legacy-merge`

This applies especially to:

1. `src/components/*`
2. `src/controllers/*`
3. `src/stores/*`
4. `src/hooks/*`
5. `src/services/*`
6. root `src/*.tsx`

## High-Probability Frontend Legacy Targets

These are classes of likely deletion targets, not the final ledger:

1. duplicate old/new domain surfaces:
   - `src/components/*` domain trees vs `src/features/*`
2. half-migrated orchestration screens:
   - `src/components/FlowsWorking.tsx`
   - `src/components/TagManager.tsx`
   - `src/components/Collections.tsx`
   - `src/components/DuplicateManager.tsx`
3. bootstrap/runtime paths that remain split between app shell, stores, and
   view hooks
4. compatibility type/adaptor shims left behind after feature moves
5. dead exports and alias modules retained only to avoid import cleanup
6. old shared folders that actually contain domain code

## Execution Sequence

1. `PBI-401` defines the target topology and folder ownership rules
2. `PBI-405` adds policy and CI guardrails
3. `PBI-406` builds the frontend legacy register and classifies current files
4. `PBI-407` executes the first deletion campaign and compatibility shim purge
5. `PBI-404` and related domain PBIs split oversized modules as part of the move/delete work

## Definition of Done

The frontend cleanup program is not done when the new folders exist.

It is done when:

1. new work cannot land in legacy paths
2. every temporary path is tracked
3. obsolete paths are deleted once replacements land
4. the root and major folders communicate ownership clearly
