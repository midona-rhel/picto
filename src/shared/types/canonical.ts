/**
 * Canonical backend contract types.
 *
 * These types match the Rust backend serialization exactly (PBI-572 naming).
 * They are the contract for all rebuilt frontend slices.
 */

// ── Entity types ─────────────────────────────────────────────────

export type EntityKind = 'single' | 'collection';

export interface CanonicalEntityGridItem {
  entity_hash: string;
  /** Hash used to load the image. For singles this equals entity_hash.
   *  For collections this is the primary member's hash (the one with a file in the blob store). */
  thumbnail_hash: string;
  entity_kind: EntityKind;
  name: string | null;
  mime_type: string;
  pixel_width: number | null;
  pixel_height: number | null;
  status: number;
  rating: number | null;
  date_added: string;
  date_created: string;
  date_modified: string;
  has_thumbnail: boolean;
  member_count: number | null;
  duration_ms: number | null;
  frame_count: number | null;
  has_audio: boolean;
  dominant_color_hex: string | null;
  size_bytes: number;
}

export interface CanonicalTagInfo {
  tag_id: number;
  namespace: string;
  subtag: string;
  source: string;
}

export interface CanonicalFolderInfo {
  folder_id: number;
  name: string;
}

export interface CanonicalEntityDetails {
  entity_hash: string;
  entity_kind: EntityKind;
  name: string | null;
  mime_type: string;
  size_bytes: number;
  pixel_width: number | null;
  pixel_height: number | null;
  duration_ms: number | null;
  frame_count: number | null;
  has_audio: boolean;
  status: number;
  rating: number | null;
  notes: string | null;
  source_urls: string[] | null;
  date_created: string;
  date_added: string;
  date_modified: string;
  dominant_color_hex: string | null;
  perceptual_hash: string | null;
  tags: CanonicalTagInfo[];
  folders: CanonicalFolderInfo[];
  member_count: number | null;
  total_size_bytes: number | null;
}

// ── Query types ──────────────────────────────────────────────────

export type ScopeKind =
  | 'system'
  | 'folder'
  | 'smart_folder'
  | 'collection'
  | 'similar'
  | 'search'
  | 'tag';

export interface BaseScope {
  kind: ScopeKind;
  key?: string | null;
  id?: number | null;
}

export type FilterOp = 'eq' | 'gte' | 'lte' | 'gt' | 'lt';

export interface RatingFilter {
  value: number;
  op?: FilterOp;
}

export type TagMatchMode = 'include' | 'exclude';

export interface TagFilter {
  tag: string;
  match_mode?: TagMatchMode;
}

export interface DateRange {
  from?: string | null;
  to?: string | null;
}

export interface QueryFilters {
  rating?: RatingFilter | null;
  colors?: string[] | null;
  mime_types?: string[] | null;
  entity_types?: string[] | null;
  tags?: TagFilter[] | null;
  date_created?: DateRange | null;
  date_added?: DateRange | null;
  date_modified?: DateRange | null;
  search_text?: string | null;
}

export interface QuerySort {
  field?: string;
  direction?: string;
}

export interface QueryPage {
  limit?: number;
  cursor?: string | null;
}

export interface EntityViewQuery {
  base_scope: BaseScope;
  filters?: QueryFilters;
  sort?: QuerySort;
  page?: QueryPage;
}

export interface EntityViewPage {
  items: CanonicalEntityGridItem[];
  next_cursor: string | null;
  total_count: number | null;
  total_size_bytes: number | null;
}

// ── Sidebar types ────────────────────────────────────────────────

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

// ── Selection types ──────────────────────────────────────────────

export interface SelectionSummary {
  total_count: number;
  entity_hashes: string[];
}

// ── Bulk target types ────────────────────────────────────────────

export type EntityTargetKind = 'entity_hashes' | 'query_results';

export interface EntityTarget {
  kind: EntityTargetKind;
  entity_hashes?: string[] | null;
  query?: EntityViewQuery | null;
  excluded_entity_hashes?: string[] | null;
}

export interface MediaEntityPatch {
  name?: string | null;
  /** Plain text notes. */
  notes?: string | null;
  rating?: number | null;
  source_urls?: string[] | null;
}
