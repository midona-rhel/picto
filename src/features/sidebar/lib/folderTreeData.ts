import type { SidebarNodeDto } from '../../../shared/types/sidebar';

export interface TreeNode extends SidebarNodeDto {
  children: TreeNode[];
  depth: number;
}

export function buildFolderTree(nodes: SidebarNodeDto[]): TreeNode[] {
  const map = new Map<string, TreeNode>();
  for (const n of nodes) {
    map.set(n.id, { ...n, children: [], depth: 0 });
  }
  const roots: TreeNode[] = [];
  for (const [, node] of map) {
    if (node.parent_id && map.has(node.parent_id)) {
      map.get(node.parent_id)!.children.push(node);
    } else {
      roots.push(node);
    }
  }
  const sortAndSetDepth = (children: TreeNode[], depth: number) => {
    children.sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
    for (const n of children) {
      n.depth = depth;
      sortAndSetDepth(n.children, depth + 1);
    }
  };
  sortAndSetDepth(roots, 0);
  return roots;
}

export function parseFolderId(nodeId: string): number | null {
  if (nodeId.startsWith('folder:')) {
    const num = parseInt(nodeId.slice('folder:'.length), 10);
    return isNaN(num) ? null : num;
  }
  return null;
}

export function getFolderAutoTags(node: SidebarNodeDto): string[] {
  const meta = node.meta as Record<string, unknown> | null | undefined;
  const raw = meta?.auto_tags;
  return Array.isArray(raw) ? raw.filter((value): value is string => typeof value === 'string') : [];
}

/** Collect all descendant node IDs (to prevent dropping a folder into its own subtree) */
export function collectDescendantIds(node: TreeNode): Set<string> {
  const ids = new Set<string>();
  const walk = (n: TreeNode) => {
    ids.add(n.id);
    for (const child of n.children) walk(child);
  };
  walk(node);
  return ids;
}

export type DropPosition = 'before' | 'inside' | 'after';

export interface DropIndicator {
  nodeId: string;
  position: DropPosition;
}
