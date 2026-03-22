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

## Program-Level Done Definition
The full program is only done when:

- raw backend access exists only in `src/platform/**` and `src/controllers/**`
- every migrated user action has one controller-owned path
- controllers own undo/redo registration for migrated domains
- long-running tasks are tracked and gated centrally
- backend emits one final self-describing `runtime/state_changed` event per completed action
- deferred media derivatives emit authoritative state changes
- frontend state reconciliation is targeted and no longer relies on broad fallback invalidation for normal correctness
- redundant near-duplicate backend and controller calls in migrated domains have been collapsed into clearer canonical operations
