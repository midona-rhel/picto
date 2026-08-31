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

  const visible = nodes
    .filter((node) => visibleIds.has(node.id))
    .map((node) => ({ ...node, sort_order: 0 }));
  const compareNames = (left: SidebarNodeDto, right: SidebarNodeDto) => (
    left.name.localeCompare(right.name, undefined, {
      sensitivity: 'base',
      numeric: true,
    })
  );
  const children = new Map<string, SidebarNodeDto[]>();
  const roots: SidebarNodeDto[] = [];
  for (const node of visible) {
    if (!node.parent_id || !visibleIds.has(node.parent_id)) {
      roots.push(node);
      continue;
    }
    const siblings = children.get(node.parent_id) ?? [];
    siblings.push(node);
    children.set(node.parent_id, siblings);
  }
  roots.sort(compareNames);
  children.forEach((siblings) => siblings.sort(compareNames));

  const ordered: SidebarNodeDto[] = [];
  const append = (node: SidebarNodeDto) => {
    ordered.push(node);
    children.get(node.id)?.forEach(append);
  };
  roots.forEach(append);
  return ordered;
}
