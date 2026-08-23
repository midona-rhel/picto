import type { FolderReorderMove } from '../../../platform/folderApi';

/**
 * Describe a folder drag as relative hash moves instead of rewriting every
 * loaded entity rank. The backend can then preserve unloaded page boundaries.
 */
export function planFolderReorder(
  orderedHashes: string[],
  draggedHashes: ReadonlySet<string>,
  dropIndex: number,
  dropSide: 'left' | 'right',
): FolderReorderMove[] {
  const dragged = orderedHashes.filter((hash) => draggedHashes.has(hash));
  if (dragged.length === 0) return [];

  const targetIndex = dropSide === 'right' ? dropIndex + 1 : dropIndex;
  const draggedBeforeTarget = orderedHashes
    .slice(0, targetIndex)
    .filter((hash) => draggedHashes.has(hash)).length;
  const remaining = orderedHashes.filter((hash) => !draggedHashes.has(hash));
  const insertAt = Math.max(0, Math.min(remaining.length, targetIndex - draggedBeforeTarget));
  const reordered = [
    ...remaining.slice(0, insertAt),
    ...dragged,
    ...remaining.slice(insertAt),
  ];
  if (reordered.every((hash, index) => hash === orderedHashes[index])) return [];

  const beforeHash = remaining[insertAt] ?? null;
  const stationaryAfterHash = insertAt > 0 ? remaining[insertAt - 1] : null;
  return dragged.map((hash, index) => ({
    hash,
    after_hash: index === 0 ? stationaryAfterHash : dragged[index - 1],
    before_hash: beforeHash,
  }));
}
