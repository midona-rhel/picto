/**
 * Frontend API layer — the only place that knows backend command names.
 * Controllers and features call these methods, never raw invoke().
 */

import { invoke } from './ipc';
import type {
  SidebarTreeResponse, EntityViewQuery, EntityViewPage,
  CanonicalEntityGridItem, BaseScope,
} from '../shared/types/canonical';

// ── Grid ────────────────────────────────────────────────────────

/**
 * Query the grid via get_grid_page_slim (legacy command) with canonical type mapping.
 *
 * The new query_entity_view command queries LibraryDatabase (new schema) which
 * may be empty if the legacy-to-new migration hasn't run for this library.
 * Using the legacy command ensures the grid works with both old and new libraries.
 *
 * TODO: Switch to query_entity_view once all libraries are migrated.
 */
export async function queryEntityView(query: EntityViewQuery): Promise<EntityViewPage> {
  const legacyQuery = {
    query: {
      scope: scopeToLegacy(query.base_scope),
      filters: {},
      sort: {
        field: query.sort?.field ?? 'date_added',
        order: query.sort?.direction ?? 'desc',
      },
      limit: query.page?.limit ?? 100,
      cursor: query.page?.cursor ?? null,
    },
  };

  console.log('[grid] request:', JSON.stringify(legacyQuery));

  const result = await invoke<{
    items: Array<Record<string, unknown>>;
    next_cursor: string | null;
    has_more: boolean;
    total_count: number | null;
  }>('get_grid_page_slim', legacyQuery).catch((err) => {
    console.error('[grid] get_grid_page_slim FAILED:', err);
    throw err;
  });

  console.log('[grid] response:', result.items.length, 'items, total:', result.total_count);

  return {
    items: result.items.map(mapLegacyGridItem),
    next_cursor: result.next_cursor,
    total_count: result.total_count,
  };
}

function scopeToLegacy(scope: BaseScope): Record<string, unknown> {
  switch (scope.kind) {
    case 'system': return { kind: 'system', system_key: scope.key === 'all' ? 'all' : scope.key };
    case 'folder': return { kind: 'folder', folder_id: scope.id };
    case 'smart_folder': return { kind: 'smart' };
    case 'collection': return { kind: 'collection', collection_entity_id: scope.id };
    default: return { kind: 'system', system_key: 'all' };
  }
}

function mapLegacyGridItem(raw: Record<string, unknown>): CanonicalEntityGridItem {
  const statusRaw = raw.status;
  let status: number;
  if (typeof statusRaw === 'number') status = statusRaw;
  else if (statusRaw === 'active') status = 1;
  else if (statusRaw === 'inbox') status = 0;
  else if (statusRaw === 'trash') status = 2;
  else status = 1;

  return {
    entity_hash: String(raw.hash ?? raw.entity_hash ?? ''),
    entity_kind: (raw.kind as string) === 'collection' ? 'collection' : 'single',
    name: (raw.name as string | null) ?? null,
    mime_type: String(raw.mime ?? raw.mime_type ?? 'application/octet-stream'),
    pixel_width: (raw.width as number | null) ?? (raw.pixel_width as number | null) ?? null,
    pixel_height: (raw.height as number | null) ?? (raw.pixel_height as number | null) ?? null,
    status,
    rating: (raw.rating as number | null) ?? null,
    date_added: String(raw.date_added ?? ''),
    date_created: String(raw.date_created ?? raw.date_added ?? ''),
    date_modified: String(raw.date_modified ?? raw.date_added ?? ''),
    has_thumbnail: raw.has_thumbnail !== false,
    member_count: (raw.member_count as number | null) ?? null,
    duration_ms: (raw.duration_ms as number | null) ?? null,
    frame_count: (raw.num_frames as number | null) ?? (raw.frame_count as number | null) ?? null,
    has_audio: raw.has_audio === true,
    dominant_color_hex: (raw.dominant_color_hex as string | null) ?? null,
    size_bytes: (raw.size as number) ?? (raw.size_bytes as number) ?? 0,
  };
}

export interface ReconcileResult {
  kind: 'no_change' | 'patch_rows' | 'replace_window' | 'full_refresh_required';
  items?: CanonicalEntityGridItem[];
  page?: EntityViewPage;
}

export function reconcileEntityView(
  query: EntityViewQuery,
  visibleHashes: string[],
  metadataOnly: boolean,
): Promise<ReconcileResult> {
  return invoke<ReconcileResult>('reconcile_entity_view', {
    query,
    visible_hashes: visibleHashes,
    metadata_only: metadataOnly,
  } as unknown as Record<string, unknown>);
}

// ── Sidebar ──────────────────────────────────────────────────────

export function getSidebarTree(): Promise<SidebarTreeResponse> {
  return invoke<SidebarTreeResponse>('get_sidebar_tree');
}

export function reorderSidebarNodes(moves: [string, number][]): Promise<void> {
  return invoke<void>('reorder_sidebar_nodes', { moves });
}

// ── Folders ──────────────────────────────────────────────────────

export function createFolder(params: {
  name: string;
  parent_id?: number | null;
  icon?: string;
  color?: string;
}): Promise<unknown> {
  return invoke('create_folder', params);
}

export function deleteFolder(folderId: number): Promise<void> {
  return invoke<void>('delete_folder', { folder_id: folderId });
}

export function renameFolder(folderId: number, name: string): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, name });
}

export function updateFolder(folderId: number, patch: {
  name?: string;
  icon?: string | null;
  color?: string | null;
}): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, ...patch });
}

export function moveFolder(
  folderId: number,
  newParentId: number | null,
  siblingOrder: [number, number][],
): Promise<void> {
  return invoke<void>('move_folder', {
    folder_id: folderId,
    new_parent_id: newParentId,
    sibling_order: siblingOrder,
  });
}

// ── Smart folders ────────────────────────────────────────────────

export function deleteSmartFolder(id: string): Promise<void> {
  return invoke<void>('delete_smart_folder', { id });
}

// NOTE: update_smart_folder requires a full SmartFolder struct.
// No partial patch command exists. Rename/color/icon for smart folders
// are blocked until the backend adds partial update support.

export function moveSmartFolder(
  smartFolderId: number,
  newParentId: number | null,
  siblingOrder: [number, number][],
): Promise<void> {
  return invoke<void>('move_smart_folder', {
    smart_folder_id: smartFolderId,
    new_parent_id: newParentId,
    sibling_order: siblingOrder,
  });
}
