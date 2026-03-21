# Delete Register

## Purpose

Track the concrete files and surfaces that must be removed, merged, or rewritten.

## Current Truth

- Legacy survives because deletion is optional and spread across too many notes.

## Target Truth

- Every known holdover is explicitly classified.

## Rename Map

- none; this file references exact current paths and their target action

## Delete List

- `src/features/settings/components/PtrPanel.tsx` — `delete`
- `src/shared/controllers/ptrSyncController.ts` — `delete`
- PTR branches inside `src/platform/api.ts` — `rewrite` to internal-only backend boundary
- PTR task UI in `src/features/layout/components/SidebarJobStatus.tsx` — `delete`
- PTR mode in `src/features/tags/components/TagManager.tsx` — `delete`
- PTR support in `src/features/tags/components/TagRelationsModal.tsx` — `delete`
- `src/shared/services/TagPickerPortal.tsx` — `merge`
- `src/features/tags/components/TagSelectPanel.tsx` — `rewrite`
- `src/shared/controllers/folderController.ts` — `delete`
- `src/shared/controllers/gridController.ts` — `delete`
- `src/shared/controllers/subscriptionController.ts` — `delete`
- `src/state/domainStore.ts` ad hoc refresh queue logic — `rewrite`
- `core/src/events.rs` compatibility events — `delete`
- `core/src/runtime_contract/mutation.rs` transitional invalidation hints — `delete`
- controller-heavy domain facades under `core/src/*/controller.rs` that only rename DB calls — `delete`
- stale audit and topology docs superseded by this tree — `archive`

## DTOs and Commands Involved

- old ptr commands
- old flow commands
- old sibling and parent commands
- old file-first logical DTO names

## Workflows

- Classify path.
- Replace last live consumer.
- Delete path in the same branch.
- Update search results and generated types before merge.

## Acceptance Criteria

- No item in this register is left ambiguous.
- Delete items are gone, merge items are folded, rewrite items have one obvious replacement.
- Searching for deleted visible PTR surfaces returns only this register or archived docs.
