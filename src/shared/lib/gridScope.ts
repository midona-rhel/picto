import type { ItemScope } from '../types/generated/application/ItemScope';

const GRID_SYSTEM_SCOPES: Record<string, ItemScope> = {
  'system:active': { kind: 'all' },
  'system:inbox': { kind: 'inbox' },
  'system:trash': { kind: 'trash' },
  'system:uncategorized': { kind: 'uncategorized' },
  'system:untagged': { kind: 'untagged' },
  'system:recent_viewed': { kind: 'recently_viewed' },
};

const NON_GRID_NODES = new Set([
  'system:duplicates',
  'system:subscriptions',
  'system:tag_manager',
]);

export function isNonGridNodeId(nodeId: string): boolean {
  return NON_GRID_NODES.has(nodeId);
}

export function nodeIdToGridScope(nodeId: string): ItemScope | null {
  if (nodeId.startsWith('folder:')) {
    const id = parseInt(nodeId.slice(7), 10);
    return { kind: 'folder', folder_id: isNaN(id) ? 0 : id };
  }
  if (nodeId.startsWith('smart:')) {
    const id = parseInt(nodeId.slice(6), 10);
    return { kind: 'smart_folder', smart_folder_id: isNaN(id) ? 0 : id };
  }
  if (NON_GRID_NODES.has(nodeId)) return null;
  const scope = GRID_SYSTEM_SCOPES[nodeId];
  if (scope) return scope;
  return null;
}

export function scopeToGridNodeId(scope: ItemScope): string | null {
  switch (scope.kind) {
    case 'all': return 'system:active';
    case 'inbox': return 'system:inbox';
    case 'trash': return 'system:trash';
    case 'uncategorized': return 'system:uncategorized';
    case 'untagged': return 'system:untagged';
    case 'recently_viewed': return 'system:recent_viewed';
    case 'folder':
      return `folder:${scope.folder_id}`;
    case 'smart_folder':
      return `smart:${scope.smart_folder_id}`;
  }
}
