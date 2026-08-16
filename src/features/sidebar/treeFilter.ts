import type { SidebarNodeDto } from '../../shared/types/canonical';

/** Keep matching nodes and their ancestry so filtered trees retain context. */
export function filterSidebarTree(nodes: SidebarNodeDto[], query: string): SidebarNodeDto[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return nodes;

  const byId = new Map(nodes.map((node) => [node.id, node]));
  const visibleIds = new Set<string>();

  for (const node of nodes) {
    if (!node.name.toLocaleLowerCase().includes(normalizedQuery)) continue;
    let current: SidebarNodeDto | undefined = node;
    while (current && !visibleIds.has(current.id)) {
      visibleIds.add(current.id);
      current = current.parent_id ? byId.get(current.parent_id) : undefined;
    }
  }

  return nodes.filter((node) => visibleIds.has(node.id));
}
