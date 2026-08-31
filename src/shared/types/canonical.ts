/**
 * Canonical backend contract types.
 *
 * These types mirror `picto_library` serialization exactly. Presentation
 * models belong at their consumer; IPC never uses the legacy item DTOs.
 */

// ── Entity types ─────────────────────────────────────────────────

export type RootKind = 'media' | 'collection';
export type Lifecycle = 'active' | 'inbox' | 'trash';
export type Rating = 'unrated' | 'one' | 'two' | 'three' | 'four' | 'five';

export interface LabColor {
  l: number;
  a: number;
  b: number;
  weight: number;
}

export interface CanonicalEntityGridItem {
  root_id: number;
  kind: RootKind;
  lifecycle: Lifecycle;
  name: string;
  cover_media_id: number;
  content_hash: string;
  mime: string;
  width: number | null;
  height: number | null;
  duration_ms: number | null;
  frame_count: number | null;
  palette: LabColor[];
  imported_at_ms: number;
  captured_at_ms: number | null;
  modified_at_ms: number;
  media_count: number;
  total_size_bytes: number;
  rating: Rating;
}

export interface MediaRecord {
  media_id: number;
  media_name: string;
  media_notes: string | null;
  file_id: number;
  file_path: string;
  facts: {
    mime: string;
    size_bytes: number;
    width: number | null;
    height: number | null;
    duration_ms: number | null;
    frame_count: number | null;
    content_hash: string;
    perceptual_hash: string | null;
    palette: LabColor[];
  };
}

export interface CanonicalEntityDetails {
  root: {
    root_id: number;
    stable_key: string;
    kind: RootKind;
    name: string;
    notes: string | null;
    source_urls: string[];
    cover_media_id: number;
    imported_at_ms: number;
    captured_at_ms: number | null;
    modified_at_ms: number;
    media_count: number;
    total_size_bytes: number;
  };
  lifecycle: Lifecycle;
  rating: Rating;
  folder_ids: number[];
  tag_ids: number[];
  media: MediaRecord[];
  revision: number;
}

export interface CanonicalTagInfo {
  tag_id: number;
  namespace: string;
  subtag: string;
}

export interface CanonicalTagRecord {
  tag_id: number;
  namespace_id: number;
  namespace: string;
  subname: string;
  active_count: number;
  assignment_count: number;
}

export interface TagPage {
  tags: CanonicalTagRecord[];
  next_cursor: string | null;
  revision: number;
}

export interface CanonicalNamespaceSummary {
  namespace_id: number;
  name: string;
  tag_count: number;
}

export interface CanonicalFolderInfo {
  folder_id: number;
  name: string;
}

export interface CanonicalFolderRecord {
  folder_id: number;
  stable_key: string;
  parent_id: number | null;
  name: string;
  icon: string | null;
  color: string | null;
  notes: string | null;
  cover_root_id: number | null;
  watch_path: string | null;
  watch_enabled: boolean;
  watch_subfolders: boolean;
  display_order: number;
  count: number;
}

export interface CanonicalSmartFolderRecord {
  smart_folder_id: number;
  name: string;
  parent_id: number | null;
  icon: string | null;
  color: string | null;
  notes: string | null;
  view: ViewQuerySpec;
  display_order: number;
  count: number;
}

export interface CanonicalNavigationSnapshot {
  folders: CanonicalFolderRecord[];
  smart_folders: CanonicalSmartFolderRecord[];
  revision: number;
}

export interface CanonicalSidebarCounts {
  all: number;
  inbox: number;
  trash: number;
  recently_viewed: number;
  untagged: number;
  uncategorized: number;
  duplicates: number;
  folders: Array<{ folder_id: number; count: number }>;
  smart_folders: Array<{ smart_folder_id: number; count: number }>;
  revision: number;
}

// ── Query types ──────────────────────────────────────────────────

export type BaseScope =
  | { kind: 'all' }
  | { kind: 'inbox' }
  | { kind: 'trash' }
  | { kind: 'media_matches'; item_id: number }
  | { kind: 'recently_viewed' }
  | { kind: 'untagged' }
  | { kind: 'uncategorized' }
  | { kind: 'folder'; folder_id: number }
  | { kind: 'smart_folder'; smart_folder_id: number };

export type QueryScope = BaseScope
  | { kind: 'folder_tree'; folder_id: number };

export type SetMatchMode = 'any' | 'all' | 'exact';
export type TextField = 'global' | 'name' | 'notes' | 'source_url';
export type FilterClause =
  | { clause: 'tags'; tag_ids: number[]; mode: SetMatchMode }
  | { clause: 'folders'; folder_ids: number[]; mode: SetMatchMode }
  | { clause: 'ratings'; ratings: Rating[] }
  | { clause: 'mime'; values: string[]; families: string[] }
  | { clause: 'imported_at'; minimum_ms: number | null; maximum_ms: number | null }
  | { clause: 'modified_at'; minimum_ms: number | null; maximum_ms: number | null }
  | { clause: 'captured_at'; minimum_ms: number | null; maximum_ms: number | null }
  | { clause: 'width'; minimum: number | null; maximum: number | null }
  | { clause: 'height'; minimum: number | null; maximum: number | null }
  | { clause: 'duration'; minimum_ms: number | null; maximum_ms: number | null }
  | { clause: 'total_size'; minimum_bytes: number | null; maximum_bytes: number | null }
  | { clause: 'notes_present'; present: boolean }
  | { clause: 'source_urls_present'; present: boolean }
  | { clause: 'color'; color: LabColor; delta_e: number }
  | { clause: 'text'; field: TextField; query: string };

export type FilterExpr =
  | { kind: 'all'; value: FilterExpr[] }
  | { kind: 'any'; value: FilterExpr[] }
  | { kind: 'not'; value: FilterExpr }
  | { kind: 'clause'; value: FilterClause };

export type SortField =
  | 'imported_at'
  | 'captured_at'
  | 'name'
  | 'rating'
  | 'total_size'
  | 'random'
  | 'folder_order';
export type SortDirection = 'ascending' | 'descending';
export interface ItemSort {
  field: SortField;
  direction: SortDirection;
  random_seed: string | null;
}
export interface ViewQuerySpec {
  filter: FilterExpr;
  sort: ItemSort;
}
export interface EntityViewQuery {
  scope: QueryScope;
  view: ViewQuerySpec;
}
export interface EntityViewPage {
  items: CanonicalEntityGridItem[];
  next_cursor: string | null;
  total: number;
  media_count: number;
  total_size_bytes: number;
  revision: number;
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

// ── Smart folder types ───────────────────────────────────────────

export type SmartFolderMatchMode = 'all' | 'any';

export interface SmartFolderPredicateRule {
  field: string;
  op: string;
  value?: unknown;
  value2?: unknown;
  values?: string[] | null;
  unit?: string;
}

export interface SmartFolderPredicateGroup {
  match_mode: SmartFolderMatchMode;
  negate?: boolean;
  rules: SmartFolderPredicateRule[];
}

export interface SmartFolderPredicate {
  groups: SmartFolderPredicateGroup[];
}

export interface SmartFolderCommandPayload {
  name: string;
  parent_id: number | null;
  icon: string | null;
  color: string | null;
  notes: string | null;
  view: ViewQuerySpec;
}

// ── Selection types ──────────────────────────────────────────────

export interface SelectionSummary {
  selected_count: number;
  taggable_root_count: number;
  total_size_bytes: number;
  media_count: number;
  shared_rating: Rating | null;
  minimum_rating: Rating | null;
  maximum_rating: Rating | null;
  shared_tags: number[];
  shared_folders: number[];
  sample_hashes: string[];
  collection_candidates: Array<{
    collection_id: number;
    label: string;
    member_count: number;
  }>;
  shared_notes: string | null;
  has_notes: boolean;
  shared_source_urls: string[] | null;
  has_source_urls: boolean;
  all_selected_roots_have_images: boolean;
  revision: number;
}

// ── Bulk target types ────────────────────────────────────────────

export type EntityTarget =
  | { kind: 'explicit'; root_ids: number[] }
  | { kind: 'query'; query: EntityViewQuery; excluded_root_ids: number[] }
  | {
      kind: 'range';
      query: EntityViewQuery;
      anchor_root_id: number;
      focus_root_id: number;
    };

export interface OrganizeCollectionInput {
  target: EntityTarget;
  cover_root_id: number;
  winning_collection_id: number | null;
  name: string | null;
  notes: string | null;
}

export interface CollectionNoteDraft {
  notes: string;
  source_count: number;
  byte_length: number;
  maximum_bytes: number;
}

export interface OrganizeCollectionResult {
  collection_id: number;
  receipt: MutationReceipt;
}

export interface DetachCollectionInput {
  collection_id: number;
  media_ids: number[];
  target_lifecycle: Lifecycle | null;
}

export interface ReorderCollectionInput {
  collection_id: number;
  media_ids: number[];
}

export interface MutationReceipt {
  revision: number;
  resources: string[];
  item_ids: number[];
}

export interface MediaEntityPatch {
  notes?: string | null;
  rating?: Rating;
  source_urls?: string[] | null;
}
