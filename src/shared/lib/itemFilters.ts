import type {
  EntityViewQuery,
  FilterExpr,
  ItemSort,
  Rating,
  SetMatchMode,
} from '../types/canonical';
import { hexToLab } from './labColor';

export interface TagFilterChoice {
  tag_id: number;
  name: string;
}

/** Editable grid-filter state. This is UI state, not an IPC DTO. */
export interface ItemFilters {
  include_tags: TagFilterChoice[];
  exclude_tags: TagFilterChoice[];
  tag_match_mode: SetMatchMode;
  include_folder_ids: number[];
  exclude_folder_ids: number[];
  folder_match_mode: SetMatchMode;
  ratings: number[];
  include_mime_types: string[];
  exclude_mime_types: string[];
  text: string | null;
  color_hex: string | null;
  imported_after: string | null;
  imported_before: string | null;
  modified_after: string | null;
  modified_before: string | null;
  min_duration_ms: bigint | null;
  max_duration_ms: bigint | null;
  min_size_bytes: bigint | null;
  max_size_bytes: bigint | null;
  min_width: bigint | null;
  max_width: bigint | null;
  min_height: bigint | null;
  max_height: bigint | null;
  notes_present: boolean | null;
  notes_contains: string | null;
  source_url_present: boolean | null;
  source_url_contains: string | null;
}

/** One canonical empty filter shape for every query owner. */
export function createEmptyItemFilters(): ItemFilters {
  return {
    include_tags: [],
    exclude_tags: [],
    tag_match_mode: 'any',
    include_folder_ids: [],
    exclude_folder_ids: [],
    folder_match_mode: 'any',
    ratings: [],
    include_mime_types: [],
    exclude_mime_types: [],
    text: null,
    color_hex: null,
    imported_after: null,
    imported_before: null,
    modified_after: null,
    modified_before: null,
    min_duration_ms: null,
    max_duration_ms: null,
    min_size_bytes: null,
    max_size_bytes: null,
    min_width: null,
    max_width: null,
    min_height: null,
    max_height: null,
    notes_present: null,
    notes_contains: null,
    source_url_present: null,
    source_url_contains: null,
  };
}

/** Exact semantic equality for the canonical filter value object. */
export function itemFiltersEqual(left: ItemFilters, right: ItemFilters): boolean {
  return JSON.stringify(left, jsonBigInt) === JSON.stringify(right, jsonBigInt);
}

export function compileGridQuery(
  scope: EntityViewQuery['scope'],
  filters: ItemFilters,
  sort: ItemSort,
  searchText = '',
): EntityViewQuery {
  const values: FilterExpr[] = [];
  const clause = (value: Extract<FilterExpr, { kind: 'clause' }>['value']): FilterExpr => ({
    kind: 'clause',
    value,
  });
  const negate = (value: FilterExpr): FilterExpr => ({ kind: 'not', value });

  if (filters.include_tags.length > 0) {
    values.push(clause({
      clause: 'tags',
      tag_ids: filters.include_tags.map((tag) => tag.tag_id),
      mode: filters.tag_match_mode,
    }));
  }
  if (filters.exclude_tags.length > 0) {
    values.push(negate(clause({
      clause: 'tags',
      tag_ids: filters.exclude_tags.map((tag) => tag.tag_id),
      mode: 'any',
    })));
  }
  if (filters.include_folder_ids.length > 0) {
    values.push(clause({
      clause: 'folders',
      folder_ids: filters.include_folder_ids,
      mode: filters.folder_match_mode,
    }));
  }
  if (filters.exclude_folder_ids.length > 0) {
    values.push(negate(clause({
      clause: 'folders',
      folder_ids: filters.exclude_folder_ids,
      mode: 'any',
    })));
  }
  if (filters.ratings.length > 0) {
    values.push(clause({ clause: 'ratings', ratings: filters.ratings.map(ratingFromNumber) }));
  }
  if (filters.include_mime_types.length > 0) {
    values.push(clause({ clause: 'mime', values: filters.include_mime_types, families: [] }));
  }
  if (filters.exclude_mime_types.length > 0) {
    values.push(negate(clause({ clause: 'mime', values: filters.exclude_mime_types, families: [] })));
  }

  addDateRange(values, 'imported_at', filters.imported_after, filters.imported_before);
  addDateRange(values, 'modified_at', filters.modified_after, filters.modified_before);
  addNumericRange(values, 'duration', filters.min_duration_ms, filters.max_duration_ms, 'minimum_ms', 'maximum_ms');
  addNumericRange(values, 'total_size', filters.min_size_bytes, filters.max_size_bytes, 'minimum_bytes', 'maximum_bytes');
  addNumericRange(values, 'width', filters.min_width, filters.max_width, 'minimum', 'maximum');
  addNumericRange(values, 'height', filters.min_height, filters.max_height, 'minimum', 'maximum');

  if (filters.notes_present != null) {
    values.push(clause({ clause: 'notes_present', present: filters.notes_present }));
  }
  if (filters.source_url_present != null) {
    values.push(clause({ clause: 'source_urls_present', present: filters.source_url_present }));
  }
  if (filters.notes_contains?.trim()) {
    values.push(clause({ clause: 'text', field: 'notes', query: filters.notes_contains.trim() }));
  }
  if (filters.source_url_contains?.trim()) {
    values.push(clause({ clause: 'text', field: 'source_url', query: filters.source_url_contains.trim() }));
  }
  const text = searchText.trim() || filters.text?.trim();
  if (text) values.push(clause({ clause: 'text', field: 'global', query: text }));
  if (filters.color_hex) {
    values.push(clause({ clause: 'color', color: hexToLab(filters.color_hex), delta_e: 12 }));
  }

  return { scope, view: { filter: { kind: 'all', value: values }, sort } };
}

function addDateRange(
  values: FilterExpr[],
  kind: 'imported_at' | 'modified_at',
  minimum: string | null,
  maximum: string | null,
): void {
  const minimumMs = parseDate(minimum);
  const maximumMs = parseDate(maximum);
  if (minimumMs == null && maximumMs == null) return;
  values.push({
    kind: 'clause',
    value: { clause: kind, minimum_ms: minimumMs, maximum_ms: maximumMs },
  });
}

function addNumericRange(
  values: FilterExpr[],
  kind: 'duration' | 'total_size' | 'width' | 'height',
  minimum: bigint | null,
  maximum: bigint | null,
  minimumKey: 'minimum_ms' | 'minimum_bytes' | 'minimum',
  maximumKey: 'maximum_ms' | 'maximum_bytes' | 'maximum',
): void {
  if (minimum == null && maximum == null) return;
  const value = {
    clause: kind,
    [minimumKey]: minimum == null ? null : Number(minimum),
    [maximumKey]: maximum == null ? null : Number(maximum),
  } as Extract<FilterExpr, { kind: 'clause' }>['value'];
  values.push({ kind: 'clause', value });
}

function parseDate(value: string | null): number | null {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function ratingFromNumber(value: number): Rating {
  return (['unrated', 'one', 'two', 'three', 'four', 'five'][value] ?? 'unrated') as Rating;
}

function jsonBigInt(_key: string, value: unknown): unknown {
  return typeof value === 'bigint' ? String(value) : value;
}
