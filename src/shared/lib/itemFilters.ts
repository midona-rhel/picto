import type { ItemFilters } from '../types/generated/application/ItemFilters';

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
