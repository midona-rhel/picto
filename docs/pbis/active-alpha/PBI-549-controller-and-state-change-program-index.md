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

## PBI Set

- [PBI-551: Centralize Long-Running Task Orchestration](./PBI-551-centralize-long-running-task-orchestration.md)
- [PBI-552: Consolidate Files Controller And Direct Entity UI Updates](./PBI-552-consolidate-files-controller-and-direct-entity-ui-updates.md)
- [PBI-553: Consolidate Tags Controller And Tag-Driven Consequences](./PBI-553-consolidate-tags-controller-and-tag-driven-consequences.md)
- [PBI-554: Consolidate Folders Controller And Folder Watch Flow](./PBI-554-consolidate-folders-controller-and-folder-watch-flow.md)
- [PBI-555: Consolidate Collections Controller](./PBI-555-consolidate-collections-controller.md)
- [PBI-556: Consolidate Smart Folders Controller And Tree Actions](./PBI-556-consolidate-smart-folders-controller-and-tree-actions.md)
- [PBI-557: Consolidate Subscriptions Controller And Window Flows](./PBI-557-consolidate-subscriptions-controller-and-window-flows.md)
- [PBI-558: Consolidate Settings, Window, Library, And Shared Platform Helpers](./PBI-558-consolidate-settings-window-library-and-shared-platform-helpers.md)
- [PBI-559: Move Undo/Redo Ownership Into Controllers](./PBI-559-move-undo-redo-ownership-into-controllers.md)
- [PBI-564: Eliminate Remaining UI-Owned Undo/Redo Registrations](./PBI-564-eliminate-remaining-ui-owned-undo-redo-registrations.md)
- [PBI-560: Finalize Runtime State-Change Contract And Combined Delta Emission](./PBI-560-finalize-runtime-state-change-contract-and-combined-delta-emission.md)
- [PBI-561: Tighten Backend Files, Tags, Media, And Import State Changes](./PBI-561-tighten-backend-files-tags-media-and-import-state-changes.md)
- [PBI-562: Tighten Backend Folders, Smart Folders, Subscriptions, And Watch State Changes](./PBI-562-tighten-backend-folders-smart-folders-subscriptions-and-watch-state-changes.md)
- [PBI-563: Consume State Changes Through Targeted Frontend Refresh And Reconciliation](./PBI-563-consume-state-changes-through-targeted-frontend-refresh-and-reconciliation.md)

## Recommended Execution Order
Recommended, not mandatory:

1. PBI-551
2. PBI-552
3. PBI-553
4. PBI-554
5. PBI-555
6. PBI-556
7. PBI-557
8. PBI-558
9. PBI-559
10. PBI-564
11. PBI-560
12. PBI-561
13. PBI-562
14. PBI-563

If an engineer can prove a better order with fewer merge conflicts and the same architectural safety, they should use it.

## Existing Partial Progress
This program is not starting from zero. Known partial progress already exists:

- raw backend access has already been reduced substantially in many frontend files
- some controller files already exist
- some `runtime/state_changed` detail work already landed
- watched-folder import and some subscription flows already batch parts of their backend deltas more cleanly than before

This should not be used as a reason to weaken the PBIs. It only means some PBIs may begin in a “partially implemented” state and should finish the slice properly.

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
