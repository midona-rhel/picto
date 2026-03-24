/**
 * Canonical backend contract types.
 *
 * These types match the Rust backend serialization exactly (PBI-572 naming).
 * They are the contract for activated frontend slices.
 *
 * Legacy types in core.ts use different field names (hash vs entity_hash,
 * mime vs mime_type, etc.) and remain for legacy consumers only.
 */

// ── Entity types ─────────────────────────────────────────────────

export type EntityKind = 'single' | 'collection';

/** Grid tile payload — matches Rust EntityGridItem serialization exactly. */
export interface CanonicalEntityGridItem {
  entity_hash: string;
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

/** Tag info as returned in entity details. */
export interface CanonicalTagInfo {
  tag_id: number;
  namespace: string;
  subtag: string;
  source: string;
}

/** Folder membership info as returned in entity details. */
export interface CanonicalFolderInfo {
  folder_id: number;
  name: string;
}

/** Inspector/detail panel payload — matches Rust EntityDetails exactly. */
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

/** Grid query model — matches Rust EntityViewQuery serialization exactly. */
export interface EntityViewQuery {
  base_scope: BaseScope;
  filters?: QueryFilters;
  sort?: QuerySort;
  page?: QueryPage;
}

/** Grid query response — matches Rust EntityViewPage serialization exactly. */
export interface EntityViewPage {
  items: CanonicalEntityGridItem[];
  next_cursor: string | null;
  total_count: number | null;
}

// ── Bulk target types ────────────────────────────────────────────

export type EntityTargetKind = 'entity_hashes' | 'query_results';

/** Bulk entity target — matches Rust EntityTarget serialization exactly. */
export interface EntityTarget {
  kind: EntityTargetKind;
  entity_hashes?: string[] | null;
  query?: EntityViewQuery | null;
  excluded_entity_hashes?: string[] | null;
}

// ── Write types ──────────────────────────────────────────────────

/** Partial metadata patch — matches Rust MediaEntityPatch serialization exactly. */
export interface MediaEntityPatch {
  name?: string | null;
  notes?: Record<string, string> | null;
  rating?: number | null;
  source_urls?: string[] | null;
}
