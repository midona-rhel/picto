import type { SmartFolder } from '../components/smart-folders/types';

export type SidebarNodeKind = 'system' | 'folder' | 'smart_folder';
export type SidebarFreshness = 'exact' | 'rebuilding' | 'stale';

export interface SidebarNodeDto {
  id: string;
  kind: SidebarNodeKind | string;
  parent_id: string | null;
  name: string;
  icon?: string | null;
  color?: string | null;
  sort_order?: number | null;
  count: number | null;
  freshness: SidebarFreshness | string;
  selectable: boolean;
  expanded_by_default?: boolean;
  meta?: Record<string, unknown> | null;
}

export interface SidebarTreeResponse {
  nodes: SidebarNodeDto[];
  tree_epoch: number;
  generated_at: string;
}

export function extractSmartFolderFromSidebarNode(node: SidebarNodeDto): SmartFolder | null {
  const meta = node.meta as Record<string, unknown> | null | undefined;
  const raw = meta?.smart_folder as Partial<SmartFolder> | undefined;
  if (!raw || typeof raw !== 'object') return null;
  if (typeof raw.name !== 'string') return null;
  return {
    id: typeof raw.id === 'string' ? raw.id : undefined,
    name: raw.name,
    icon: typeof raw.icon === 'string' || raw.icon === null ? raw.icon : null,
    color: typeof raw.color === 'string' || raw.color === null ? raw.color : null,
    predicate: (raw.predicate as SmartFolder['predicate']) ?? { groups: [] },
    sort_field: typeof raw.sort_field === 'string' || raw.sort_field === null ? raw.sort_field : null,
    sort_order: typeof raw.sort_order === 'string' || raw.sort_order === null ? raw.sort_order : null,
    created_at: typeof raw.created_at === 'string' || raw.created_at === null ? raw.created_at : null,
    updated_at: typeof raw.updated_at === 'string' || raw.updated_at === null ? raw.updated_at : null,
  };
}

