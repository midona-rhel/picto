import type { BaseScope } from '../types/canonical';

const GRID_SYSTEM_SCOPES: Record<string, string> = {
  'system:active': 'all',
  'system:inbox': 'inbox',
  'system:trash': 'trash',
  'system:uncategorized': 'uncategorized',
  'system:untagged': 'untagged',
};

const NON_GRID_NODES = new Set([
  'system:duplicates',
  'system:recent_viewed',
  'system:subscriptions',
  'system:tag_manager',
]);

export function isNonGridNodeId(nodeId: string): boolean {
  return NON_GRID_NODES.has(nodeId);
}

export function nodeIdToGridScope(nodeId: string): BaseScope | null {
  if (nodeId.startsWith('folder:')) {
    const id = parseInt(nodeId.slice(7), 10);
    return { kind: 'folder', id: isNaN(id) ? 0 : id };
  }
  if (nodeId.startsWith('smart:')) {
    const id = parseInt(nodeId.slice(6), 10);
    return { kind: 'smart_folder', id: isNaN(id) ? 0 : id };
  }
  if (nodeId.startsWith('collection:')) {
    const id = parseInt(nodeId.slice(11), 10);
    return { kind: 'collection', id: isNaN(id) ? 0 : id };
  }
  if (NON_GRID_NODES.has(nodeId)) return null;
  const scopeKey = GRID_SYSTEM_SCOPES[nodeId];
  if (scopeKey) return { kind: 'system', key: scopeKey };
  return null;
}

export function scopeToGridNodeId(scope: BaseScope): string | null {
  switch (scope.kind) {
    case 'system':
      return `system:${scope.key === 'all' ? 'active' : scope.key}`;
    case 'folder':
      return scope.id != null ? `folder:${scope.id}` : null;
    case 'smart_folder':
      return scope.id != null ? `smart:${scope.id}` : null;
    case 'collection':
      return scope.id != null ? `collection:${scope.id}` : null;
    default:
      return null;
  }
}
