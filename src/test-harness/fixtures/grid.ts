/**
 * Grid page fixtures.
 *
 * Covers: mixed content (images, videos, collections), pagination states,
 * empty results, single-page vs multi-page, various sort/filter combos.
 * Uses canonical EntityViewPage / CanonicalEntityGridItem types.
 */

import type {
  CanonicalEntityGridItem,
  EntityViewPage,
  EntityViewQuery,
} from '../../shared/types/canonical';

// ── Individual items ─────────────────────────────────────────────

export const jpegImage: CanonicalEntityGridItem = {
  entity_hash: 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
  entity_kind: 'single',
  name: 'sunset_beach.jpg',
  mime_type: 'image/jpeg',
  pixel_width: 3840,
  pixel_height: 2160,
  status: 1,
  rating: 5,
  date_added: '2026-03-20T14:30:00Z',
  date_created: '2026-03-15T09:00:00Z',
  date_modified: '2026-03-20T14:30:00Z',
  has_thumbnail: true,
  member_count: null,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  dominant_color_hex: '#e8731a',
  size_bytes: 4_200_000,
};

export const pngImage: CanonicalEntityGridItem = {
  entity_hash: 'b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3',
  entity_kind: 'single',
  name: 'character_design_v3.png',
  mime_type: 'image/png',
  pixel_width: 2048,
  pixel_height: 2048,
  status: 1,
  rating: null,
  date_added: '2026-03-21T10:00:00Z',
  date_created: '2026-03-21T10:00:00Z',
  date_modified: '2026-03-21T10:00:00Z',
  has_thumbnail: true,
  member_count: null,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  dominant_color_hex: '#3f51b5',
  size_bytes: 8_100_000,
};

export const videoMp4: CanonicalEntityGridItem = {
  entity_hash: 'c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4',
  entity_kind: 'single',
  name: 'timelapse_clouds.mp4',
  mime_type: 'video/mp4',
  pixel_width: 1920,
  pixel_height: 1080,
  status: 1,
  rating: 3,
  date_added: '2026-03-19T08:00:00Z',
  date_created: '2026-03-10T16:00:00Z',
  date_modified: '2026-03-19T08:00:00Z',
  has_thumbnail: true,
  member_count: null,
  duration_ms: 32_400,
  frame_count: 810,
  has_audio: false,
  dominant_color_hex: '#81d4fa',
  size_bytes: 24_500_000,
};

export const videoWithAudio: CanonicalEntityGridItem = {
  entity_hash: 'd4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5',
  entity_kind: 'single',
  name: 'interview_clip.mp4',
  mime_type: 'video/mp4',
  pixel_width: 1280,
  pixel_height: 720,
  status: 1,
  rating: null,
  date_added: '2026-03-22T12:00:00Z',
  date_created: '2026-03-22T11:30:00Z',
  date_modified: '2026-03-22T12:00:00Z',
  has_thumbnail: true,
  member_count: null,
  duration_ms: 180_000,
  frame_count: 5400,
  has_audio: true,
  dominant_color_hex: null,
  size_bytes: 95_000_000,
};

export const animatedGif: CanonicalEntityGridItem = {
  entity_hash: 'e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6',
  entity_kind: 'single',
  name: 'loading_spinner.gif',
  mime_type: 'image/gif',
  pixel_width: 256,
  pixel_height: 256,
  status: 1,
  rating: null,
  date_added: '2026-03-18T09:00:00Z',
  date_created: '2026-03-18T09:00:00Z',
  date_modified: '2026-03-18T09:00:00Z',
  has_thumbnail: true,
  member_count: null,
  duration_ms: 2000,
  frame_count: 24,
  has_audio: false,
  dominant_color_hex: '#ffffff',
  size_bytes: 120_000,
};

export const collection: CanonicalEntityGridItem = {
  entity_hash: 'f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1',
  entity_kind: 'collection',
  name: 'Beach Vacation 2026',
  mime_type: 'image/jpeg',
  pixel_width: 3840,
  pixel_height: 2160,
  status: 1,
  rating: 4,
  date_added: '2026-03-20T14:30:00Z',
  date_created: '2026-03-15T09:00:00Z',
  date_modified: '2026-03-20T14:35:00Z',
  has_thumbnail: true,
  member_count: 47,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  dominant_color_hex: '#e8731a',
  size_bytes: 185_000_000,
};

export const noThumbnail: CanonicalEntityGridItem = {
  entity_hash: 'a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8c9d0e1f2a7b8',
  entity_kind: 'single',
  name: 'corrupted_import.png',
  mime_type: 'image/png',
  pixel_width: null,
  pixel_height: null,
  status: 0,
  rating: null,
  date_added: '2026-03-23T16:00:00Z',
  date_created: '2026-03-23T16:00:00Z',
  date_modified: '2026-03-23T16:00:00Z',
  has_thumbnail: false,
  member_count: null,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  dominant_color_hex: null,
  size_bytes: 0,
};

export const noName: CanonicalEntityGridItem = {
  entity_hash: 'b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9d0e1f2a3b8c9',
  entity_kind: 'single',
  name: null,
  mime_type: 'image/jpeg',
  pixel_width: 800,
  pixel_height: 600,
  status: 1,
  rating: null,
  date_added: '2026-03-24T08:00:00Z',
  date_created: '2026-03-24T08:00:00Z',
  date_modified: '2026-03-24T08:00:00Z',
  has_thumbnail: true,
  member_count: null,
  duration_ms: null,
  frame_count: null,
  has_audio: false,
  dominant_color_hex: '#888888',
  size_bytes: 350_000,
};

// ── Assembled pages ──────────────────────────────────────────────

/** Standard first page with mixed content. */
export const gridPageMixed: EntityViewPage = {
  items: [jpegImage, pngImage, videoMp4, animatedGif, collection, videoWithAudio, noName],
  next_cursor: '2026-03-18T09:00:00Z|e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6',
  total_count: 1247,
};

/** Empty results (no matching items). */
export const gridPageEmpty: EntityViewPage = {
  items: [],
  next_cursor: null,
  total_count: 0,
};

/** Single item result. */
export const gridPageSingle: EntityViewPage = {
  items: [jpegImage],
  next_cursor: null,
  total_count: 1,
};

/** Last page (no more pages). */
export const gridPageLast: EntityViewPage = {
  items: [noName],
  next_cursor: null,
  total_count: 1247,
};

/** Inbox items (status=0). */
export const gridPageInbox: EntityViewPage = {
  items: [noThumbnail],
  next_cursor: null,
  total_count: 1,
};

// ── Matching queries ─────────────────────────────────────────────

export const queryAllActive: EntityViewQuery = {
  base_scope: { kind: 'system', key: 'all' },
  sort: { field: 'date_added', direction: 'desc' },
  page: { limit: 100 },
};

export const queryInbox: EntityViewQuery = {
  base_scope: { kind: 'system', key: 'inbox' },
  sort: { field: 'date_added', direction: 'desc' },
  page: { limit: 100 },
};

export const queryFolder: EntityViewQuery = {
  base_scope: { kind: 'folder', id: 1 },
  sort: { field: 'date_added', direction: 'desc' },
  page: { limit: 100 },
};

export const queryWithFilters: EntityViewQuery = {
  base_scope: { kind: 'system', key: 'all' },
  filters: {
    rating: { value: 3, op: 'gte' },
    entity_types: ['image'],
    tags: [{ tag: 'landscape', match_mode: 'include' }],
  },
  sort: { field: 'rating', direction: 'desc' },
  page: { limit: 50 },
};

export const querySearch: EntityViewQuery = {
  base_scope: { kind: 'search', key: 'sunset' },
  sort: { field: 'date_added', direction: 'desc' },
  page: { limit: 100 },
};
