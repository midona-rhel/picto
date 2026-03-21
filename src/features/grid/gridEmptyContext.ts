import type { GridEmptyContext } from './runtime';
import type { SmartFolderPredicate } from '../../features/smart-folders/components/types';

export function resolveGridEmptyContext(
  smartFolderPredicate: SmartFolderPredicate | null | undefined,
  folderId: number | null | undefined,
  statusFilter: string | null | undefined,
): GridEmptyContext {
  if (smartFolderPredicate) return 'smart-folder';
  if (folderId) return 'folder';
  if (statusFilter === 'inbox') return 'inbox';
  if (statusFilter === 'uncategorized') return 'uncategorized';
  if (statusFilter === 'untagged') return 'untagged';
  return 'default';
}
