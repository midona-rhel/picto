/**
 * Inspector / entity details / selection fixtures.
 *
 * Covers: single image details, video details, collection details,
 * selection summaries for single/multi/virtual-select-all,
 * entity with rich metadata vs sparse metadata.
 */

import type {
  CanonicalEntityDetails,
  SelectionSummary,
} from '../../shared/types/canonical';

// ── Entity details ───────────────────────────────────────────────

/** Fully populated image with tags, folders, notes, source URLs, rating. */
export const detailsRichImage: CanonicalEntityDetails = {
  entity_hash: 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
  thumbnail_hash: 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
  entity_kind: 'single',
  name: 'sunset_beach.jpg',
  mime_type: 'image/jpeg',
  size_bytes: 4_200_000,
  pixel_width: 3840,
  pixel_height: 2160,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  status: 1,
  rating: 5,
  notes: '{"comment":"Golden hour shot from Malibu","edit_log":"Cropped and color-corrected"}',
  source_urls: [
    'https://example.com/gallery/sunset-beach',
    'https://example.com/portfolio/landscapes',
  ],
  date_created: '2026-03-15T09:00:00Z',
  date_added: '2026-03-20T14:30:00Z',
  date_modified: '2026-03-22T11:00:00Z',
  dominant_color_hex: '#e8731a',
  dominant_colors: [
    { hex: '#e8731a', l: 62.1, a: 31.2, b: 58.4 },
    { hex: '#2f5f87', l: 41.8, a: -3.1, b: -24.6 },
    { hex: '#f4d9b6', l: 88.5, a: 4.2, b: 22.8 },
  ],
  perceptual_hash: 'AQAAAA==',
  tags: [
    { tag_id: 1, namespace: '', subtag: 'landscape', site_mask: '0', provenance_mask: '1', source: 'local' },
    { tag_id: 2, namespace: '', subtag: 'sunset', site_mask: '0', provenance_mask: '1', source: 'local' },
    { tag_id: 3, namespace: '', subtag: 'beach', site_mask: '0', provenance_mask: '1', source: 'local' },
    { tag_id: 10, namespace: 'creator', subtag: 'photographer_name', site_mask: '0', provenance_mask: '1', source: 'local' },
    { tag_id: 20, namespace: 'meta', subtag: 'high_quality', site_mask: '0', provenance_mask: '2', source: 'ai_tagger' },
  ],
  folders: [
    { folder_id: 1, name: 'Artwork' },
    { folder_id: 5, name: 'Downloads' },
  ],
  member_count: null,
  total_size_bytes: null,
};

/** Minimal image — no tags, no folders, no notes, no rating. */
export const detailsSparseImage: CanonicalEntityDetails = {
  entity_hash: 'b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9',
  thumbnail_hash: 'b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9',
  entity_kind: 'single',
  name: null,
  mime_type: 'image/jpeg',
  size_bytes: 350_000,
  pixel_width: 800,
  pixel_height: 600,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  status: 1,
  rating: null,
  notes: null,
  source_urls: null,
  date_created: '2026-03-24T08:00:00Z',
  date_added: '2026-03-24T08:00:00Z',
  date_modified: '2026-03-24T08:00:00Z',
  dominant_color_hex: '#888888',
  dominant_colors: [{ hex: '#888888', l: 57.0, a: 0.0, b: 0.0 }],
  perceptual_hash: null,
  tags: [],
  folders: [],
  member_count: null,
  total_size_bytes: null,
};

/** Video with audio, duration, frame count. */
export const detailsVideo: CanonicalEntityDetails = {
  entity_hash: 'c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4',
  thumbnail_hash: 'c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4',
  entity_kind: 'single',
  name: 'timelapse_clouds.mp4',
  mime_type: 'video/mp4',
  size_bytes: 24_500_000,
  pixel_width: 1920,
  pixel_height: 1080,
  duration_ms: 32_400,
  frame_count: 810,
  has_audio: false,
  status: 1,
  rating: 3,
  notes: null,
  source_urls: null,
  date_created: '2026-03-10T16:00:00Z',
  date_added: '2026-03-19T08:00:00Z',
  date_modified: '2026-03-19T08:00:00Z',
  dominant_color_hex: '#81d4fa',
  dominant_colors: null,
  perceptual_hash: 'BQAAAA==',
  tags: [
    { tag_id: 5, namespace: '', subtag: 'timelapse', site_mask: '0', provenance_mask: '1', source: 'local' },
    { tag_id: 6, namespace: '', subtag: 'nature', site_mask: '0', provenance_mask: '1', source: 'local' },
  ],
  folders: [],
  member_count: null,
  total_size_bytes: null,
};

/** Collection with 47 members. */
export const detailsCollection: CanonicalEntityDetails = {
  entity_hash: 'f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1',
  thumbnail_hash: 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
  entity_kind: 'collection',
  name: 'Beach Vacation 2026',
  mime_type: 'image/jpeg',
  size_bytes: 185_000_000,
  pixel_width: 3840,
  pixel_height: 2160,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  status: 1,
  rating: 4,
  notes: '{"description":"Photos from spring break trip"}',
  source_urls: null,
  date_created: '2026-03-15T09:00:00Z',
  date_added: '2026-03-20T14:30:00Z',
  date_modified: '2026-03-20T14:35:00Z',
  dominant_color_hex: '#e8731a',
  dominant_colors: null,
  perceptual_hash: null,
  tags: [
    { tag_id: 3, namespace: '', subtag: 'beach', site_mask: '0', provenance_mask: '1', source: 'local' },
    { tag_id: 7, namespace: '', subtag: 'vacation', site_mask: '0', provenance_mask: '1', source: 'local' },
  ],
  folders: [
    { folder_id: 1, name: 'Artwork' },
  ],
  member_count: 47,
  total_size_bytes: 185_000_000,
};

/** Inbox item (status=0, pending review). */
export const detailsInboxItem: CanonicalEntityDetails = {
  entity_hash: 'a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8',
  thumbnail_hash: 'a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8',
  entity_kind: 'single',
  name: 'corrupted_import.png',
  mime_type: 'image/png',
  size_bytes: 0,
  pixel_width: null,
  pixel_height: null,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  status: 0,
  rating: null,
  notes: null,
  source_urls: null,
  date_created: '2026-03-23T16:00:00Z',
  date_added: '2026-03-23T16:00:00Z',
  date_modified: '2026-03-23T16:00:00Z',
  dominant_color_hex: null,
  dominant_colors: null,
  perceptual_hash: null,
  tags: [],
  folders: [],
  member_count: null,
  total_size_bytes: null,
};

// ── Selection summaries ──────────────────────────────────────────

const EMPTY_STATS: SelectionSummary['stats'] = {
  total_size_bytes: null,
  mime_counts: null,
  rating_stats: null,
};

/** Single entity selected. */
export const selectionSingle: SelectionSummary = {
  total_count: 1,
  selected_count: 1,
  sample_hashes: ['a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2'],
  shared_tags: [],
  top_tags: [],
  shared_folders: [],
  stats: EMPTY_STATS,
  pending: false,
  generated_at: '2026-03-28T00:00:00Z',
};

/** Three entities explicitly selected. */
export const selectionMulti: SelectionSummary = {
  total_count: 3,
  selected_count: 3,
  sample_hashes: [
    'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
    'b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3',
    'c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4',
  ],
  shared_tags: [],
  top_tags: [],
  shared_folders: [],
  stats: EMPTY_STATS,
  pending: false,
  generated_at: '2026-03-28T00:00:00Z',
};

/** Virtual select-all (query_results target — sample hashes are a subset). */
export const selectionVirtualAll: SelectionSummary = {
  total_count: 1247,
  selected_count: 1247,
  sample_hashes: [],
  shared_tags: [],
  top_tags: [],
  shared_folders: [],
  stats: EMPTY_STATS,
  pending: false,
  generated_at: '2026-03-28T00:00:00Z',
};

/** Nothing selected. */
export const selectionEmpty: SelectionSummary = {
  total_count: 0,
  selected_count: 0,
  sample_hashes: [],
  shared_tags: [],
  top_tags: [],
  shared_folders: [],
  stats: EMPTY_STATS,
  pending: false,
  generated_at: '2026-03-28T00:00:00Z',
};
