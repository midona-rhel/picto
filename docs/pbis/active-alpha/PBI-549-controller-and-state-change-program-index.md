# PBI-549: Controller And State-Change Program Index

## AI-Generated Caveat
This PBI set was generated from an in-repo audit plus interactive review of the current codebase on 2026-03-22. It is intentionally concrete, but it is still AI-generated planning. Every PBI below should be treated as:

- a strong implementation brief
- not a guarantee that every edge case has already been discovered
- something the implementing engineer should challenge, tighten, and improve while working

If an engineer finds a simpler and better cut while preserving the same architectural outcome, they should take it and update the PBI notes in the same change.

## Program Goal
Replace the current mixed backend access and coarse refresh model with one coherent architecture:

- frontend code only talks to domain controllers
- controllers own eager visible updates, command/query calls, and undo/redo registration
- long-running tasks are tracked and gated centrally
- backend emits one final `runtime/state_changed` event per completed action
- `runtime/state_changed` is self-describing enough that the frontend can update the right state without broad “invalidate everything” fallbacks

## Program Rules

### 1. PBIs must be atomic
Each PBI in this set is intended to be independently executable. Do not bundle two PBIs together just because they touch nearby files.

The only acceptable reasons to combine work are:

- one PBI is blocked on a missing primitive from another
- a single refactor physically cannot land safely in two separate slices
- a shared test/helper is genuinely required by both and is small

Otherwise, complete one PBI, validate it, then move on.

### 2. Every PBI must look for adjacent cleanup
The engineer implementing a PBI must actively look for:

- duplicated code in the same slice
- inconsistent naming in the same slice
- overly broad state updates in the same slice
- controller methods that should collapse into fewer, clearer methods
- backend state-change details that are still too vague for the affected slice
- near-duplicate API or controller calls that differ only by tiny input-shape or naming variations

If two calls are effectively the same operation with slightly different parameter spelling, target type, or naming, prefer collapsing them into one clearer API/controller surface instead of preserving both.

### 2a. Prefer function-led naming, not redundant implementation-led naming
Within a domain controller, method names should describe the action once.

Good:

- `foldersController.create(...)`
- `foldersController.update(...)`
- `foldersController.delete(...)`
- `smartFoldersController.move(...)`
- `collectionsController.addMembers(...)`

Bad:

- `foldersController.createFolder(...)`
- `foldersController.updateFolder(...)`
- `foldersController.deleteFolder(...)`
- `smartFoldersController.moveSmartFolder(...)`

The domain is already expressed by the controller name. Do not repeat it in every method unless it disambiguates a genuinely different aggregate inside the same controller.

Do not reopen unrelated domains. Do improve the local slice if the improvement is clearly part of the same architectural problem.

### 3. Completion requires proof
A PBI is not done because code compiled. It is only done when:

- its boundary is enforced
- its direct UI behavior works
- its state-change path is correct
- its undo/redo path is correct if applicable
- its focused tests pass
- its manual validation checklist passes
- its public naming is clearer than what it replaced

## Reset Lifecycle Model
For the greenfield reset line starting at `PBI-567`, every PBI should be tracked in four states instead of one vague “done/not done” bucket.

### 1. Implemented
Use this when:
- the replacement layer exists
- its local boundary is real
- its direct code/tests for that slice are in place as far as currently possible
- remaining blockers are outside the slice and explicitly named

### 2. Activatable
Use this when:
- the PBI itself is implemented
- every direct dependency listed on that PBI is implemented
- anything still missing is a known cross-PBI TODO, not local architectural confusion

### 3. Activated
Use this when:
- the product is using the new path by default for the intended flow
- dependent layers are actually calling it end to end
- runtime/manual verification for the active path has happened

### 4. Legacy Removed
Use this when:
- the old path is deleted
- old names/helpers/handlers for that replaced slice are gone
- the team is no longer carrying both paths for that slice

Do not force early reset PBIs to remain “open” forever just because later PBIs are needed before activation. Mark them `Implemented`, then `Activatable`, then `Activated`, then `Legacy Removed`.

## Reset Program Order
Use this as the main activation path for the reset set:

1. Activate the rule prerequisites:
- `PBI-572` naming
- `PBI-579` testing
- `PBI-580` comment discipline

2. Implement the storage and engine foundations:
- `PBI-567` library database
- `PBI-568` backend engine
- `PBI-578` bulk entity target and selection

3. Lock the frontend architecture and styling contracts:
- `PBI-588`
- `PBI-594`

4. Lock the frontend rebuild boundary and quarantine the old frontend:
- `PBI-588`
- `PBI-589`
- `PBI-590`

5. Rebuild the core entity flow in two aligned tracks:
- Track A: `PBI-591`, `PBI-592`, `PBI-593`
- Track B: `PBI-581`, `PBI-582`, `PBI-587`, `PBI-583`, `PBI-584`, plus the matching backend contract work in `PBI-568` and `PBI-578`
- then activate `PBI-567`, `PBI-568`, and `PBI-578` together for core entity reads/writes once the rebuilt shell/grid/inspector slices and the query/selection/state gates are met

6. Activate media and long-running platform layers:
- `PBI-569` media delivery
- `PBI-585` frontend media consumption
- `PBI-576` deferred work/background processing

7. Activate ingest and downstream subsystems on top of the new foundations:
- `PBI-573` ingest/import
- `PBI-574` export jobs
- `PBI-575` subscriptions
- `PBI-577` duplicates/rejected-media

8. Activate frontend structure cleanup after the boundary is stable:
- `PBI-586` frontend feature/module architecture
- `PBI-571` frontend shared component/styling system

Some implementation work can overlap, but activation should generally follow this order.

## Program Status
This document is now a retained program summary, not a child-index.

The original controller/state-change child PBIs that used to sit under this program were completed and removed from `active-alpha` as they were closed. Do not treat missing historical child PBIs as missing work by default. If new follow-up work is discovered, create a fresh PBI for that specific gap instead of resurrecting the old link farm.

What already landed from this program:

- raw backend access was pushed behind controller/platform boundaries
- controller ownership was expanded across the main frontend domains
- long-running task orchestration was centralized
- `runtime/state_changed` became the main committed backend state event
- broad fallback refresh paths were reduced in favor of targeted reconciliation
- folder/smart-folder/subscription/watch state-change payloads were tightened and cleaned up

What this file is for now:

- preserve the architectural intent of the program
- define the bar for future related PBIs
- prevent the codebase from drifting back toward mixed backend access and vague refresh behavior

For the greenfield reset set that starts at `PBI-567`, [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md) is the naming prerequisite, [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md) is the testing prerequisite, and [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md) is the comment-discipline prerequisite. Do not treat naming, test shape, or comment discipline as cleanup after the fact in that reset line.

## Program-Level Done Definition
The full program is only done when:

- raw backend access exists only in `src/platform/**` and `src/controllers/**`
- every migrated user action has one controller-owned path
- controllers own undo/redo registration for migrated domains
- migrated frontend state ownership is explicit and no longer duplicated across nearby hooks/components
- long-running tasks are tracked and gated centrally
- backend emits one final self-describing `runtime/state_changed` event per completed action
- deferred media derivatives emit authoritative state changes
- frontend state reconciliation is targeted and no longer relies on broad fallback invalidation for normal correctness
- redundant near-duplicate backend and controller calls in migrated domains have been collapsed into clearer canonical operations
