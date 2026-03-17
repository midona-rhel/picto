import type { SmartFolderPredicate } from '../../../shared/types/api';

export interface SmartFolderTreeNode {
  id: string;
  name: string;
  parent_id: string | null;
  display_order?: number | null;
  icon?: string | null;
  color?: string | null;
  count: number;
  freshness: string;
  predicate?: SmartFolderPredicate;
  localPredicate?: SmartFolderPredicate;
  hasEffectiveRules: boolean;
  hasLocalRules: boolean;
  sort_field?: string | null;
  sort_order?: string | null;
  children: SmartFolderTreeNode[];
  depth: number;
}

export function buildSmartFolderTree(nodes: Omit<SmartFolderTreeNode, 'children' | 'depth'>[]): SmartFolderTreeNode[] {
  const map = new Map<string, SmartFolderTreeNode>();
  for (const node of nodes) {
    map.set(node.id, { ...node, children: [], depth: 0 });
  }
  const roots: SmartFolderTreeNode[] = [];
  for (const node of map.values()) {
    if (node.parent_id && map.has(node.parent_id)) {
      map.get(node.parent_id)!.children.push(node);
    } else {
      roots.push(node);
    }
  }
  const sortAndSetDepth = (items: SmartFolderTreeNode[], depth: number) => {
    items.sort((a, b) => {
      const aOrder = a.display_order ?? Number.MAX_SAFE_INTEGER;
      const bOrder = b.display_order ?? Number.MAX_SAFE_INTEGER;
      if (aOrder !== bOrder) return aOrder - bOrder;
      return a.id.localeCompare(b.id, undefined, { numeric: true });
    });
    for (const item of items) {
      item.depth = depth;
      sortAndSetDepth(item.children, depth + 1);
    }
  };
  sortAndSetDepth(roots, 0);
  return roots;
}

export function collectSmartFolderDescendantIds(node: SmartFolderTreeNode): Set<string> {
  const ids = new Set<string>();
  const walk = (current: SmartFolderTreeNode) => {
    ids.add(current.id);
    for (const child of current.children) walk(child);
  };
  walk(node);
  return ids;
}

export type SmartFolderDropPosition = 'before' | 'inside' | 'after';

export interface SmartFolderDropIndicator {
  nodeId: string;
  position: SmartFolderDropPosition;
}
