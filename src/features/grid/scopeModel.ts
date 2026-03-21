export interface ActiveGridScopeInput {
  currentView: string;
  activeFolderId: number | null;
  activeCollectionId: number | null;
  activeSmartFolderId: string | null;
  activeStatusFilter: string | null;
}

export interface ActiveGridScopeCountInput {
  activeFolderId: number | null;
  activeCollectionId: number | null;
  activeSmartFolderId: string | null;
  activeStatusFilter: string | null;
  activeFolderCount: number | null;
  allImagesCount: number | null;
  inboxCount: number | null;
  uncategorizedCount: number | null;
  untaggedCount: number | null;
  trashCount: number | null;
  smartFolderCounts: Record<string, number>;
}

export function deriveGridScopeKey(input: ActiveGridScopeInput): string | null {
  if (input.currentView !== 'images') return null;
  if (input.activeCollectionId != null) return `collection:${input.activeCollectionId}`;
  if (input.activeFolderId != null) return `folder:${input.activeFolderId}`;
  if (input.activeSmartFolderId) return `smart:${input.activeSmartFolderId}`;
  if (input.activeStatusFilter === 'random') return 'system:all';
  if (input.activeStatusFilter) return `system:${input.activeStatusFilter}`;
  return 'system:all';
}

export function deriveActiveGridScopeCount(input: ActiveGridScopeCountInput): number | null {
  const statusFilterCount =
    input.activeStatusFilter === 'inbox' ? input.inboxCount
    : input.activeStatusFilter === 'uncategorized' ? input.uncategorizedCount
    : input.activeStatusFilter === 'untagged' ? input.untaggedCount
    : input.activeStatusFilter === 'trash' ? input.trashCount
    : null;

  if (input.activeFolderId != null) return input.activeFolderCount;
  if (input.activeCollectionId != null) return null;
  if (input.activeSmartFolderId) return input.smartFolderCounts[input.activeSmartFolderId] ?? null;
  return statusFilterCount ?? input.allImagesCount;
}
