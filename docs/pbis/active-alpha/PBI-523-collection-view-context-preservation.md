# PBI-523: Collection view context preservation + breadcrumb

## Priority
P2

## Problem

When navigating to a collection (Edit Collection), `navigateToCollection` in `navigationStore.ts` creates a new history entry that clears `folderId`, `smartFolderId`, and `statusFilter`. This means:

1. The sidebar loses its highlight — it doesn't know which scope the user came from
2. The collection appears to be in the "All" scope, even if the user was in Inbox, a folder, or a smart folder
3. There's no visual indication of what collection is being viewed or how to navigate back

## Current behavior

```
User is in "Inbox" (sidebar highlights Inbox)
  → Right-clicks collection → Edit Collection
    → navigateToCollection() creates HistoryEntry with:
      view: 'images', collectionId: 42,
      folderId: null, smartFolderId: null, statusFilter: null
    → Sidebar highlights nothing (or falls back to "All")
    → No breadcrumb shows "Inbox > My Collection"
```

## Desired behavior

```
User is in "Inbox" (sidebar highlights Inbox)
  → Right-clicks collection → Edit Collection
    → navigateToCollection() preserves parent scope:
      view: 'images', collectionId: 42,
      folderId: null, smartFolderId: null, statusFilter: 'inbox'  ← preserved
    → Sidebar still highlights "Inbox"
    → Breadcrumb shows "Inbox > My Collection"
    → Pressing back returns to Inbox view
```

## Implementation notes

### 1. `navigateToCollection` — preserve parent scope
**File:** `src/state/navigationStore.ts`

Instead of clearing all scope fields:
```typescript
const entry: HistoryEntry = {
  view: 'images',
  smartFolderId: state.activeSmartFolderId,  // preserve
  folderId: state.activeFolderId,            // preserve
  collectionId,
  statusFilter: state.activeStatusFilter,    // preserve
  filterTags: state.filterTags,              // preserve
  ...
};
```

### 2. Sidebar highlight — use parent scope when collection is active
**File:** `src/features/sidebar/components/Sidebar.tsx`

The sidebar item highlight logic checks `activeFolderId`, `activeSmartFolderId`, `activeStatusFilter`. If `activeCollectionId` is set, these fields now carry the parent scope, so the sidebar naturally highlights the correct item.

### 3. Breadcrumb UI
**File:** `src/features/layout/components/ImageGridControls.tsx` (or new component)

Show a breadcrumb like `Inbox > My Collection` or `Folder Name > Collection Name`. Clicking the parent part navigates back. This requires:
- Knowing the parent scope label (from sidebar nodes)
- Knowing the collection name (from `collectionTitles` cache in navigationStore)
- A simple `ParentScope / CollectionName` display with click-to-navigate-back

### 4. Grid query unchanged
The collection scope query only uses `collectionId` — it ignores `folderId`/`statusFilter`. So preserving those in the history entry doesn't affect the grid data.

## Files affected

| File | Change |
|------|---------|
| `src/state/navigationStore.ts` | Preserve parent scope in `navigateToCollection` |
| `src/features/sidebar/components/Sidebar.tsx` | May need minor tweak for highlight logic |
| `src/features/layout/components/ImageGridControls.tsx` | Breadcrumb display |

## Acceptance criteria

- [ ] Navigate to collection from Inbox → sidebar highlights Inbox
- [ ] Navigate to collection from a folder → sidebar highlights that folder
- [ ] Navigate to collection from All → sidebar highlights All
- [ ] Breadcrumb shows "Parent > Collection Name"
- [ ] Clicking breadcrumb parent navigates back
- [ ] Back button works correctly (returns to parent scope)
- [ ] Grid shows correct collection contents regardless of parent scope
