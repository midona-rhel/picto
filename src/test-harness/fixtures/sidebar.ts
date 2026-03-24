/**
 * Sidebar tree fixtures.
 *
 * Matches the LIVE sidebar contract from core/src/sidebar/compiler.rs
 * and core/src/sidebar/db.rs (seed_sidebar_if_empty).
 *
 * Key contract details:
 *   - System scope IDs: system:active (not system:all), system:inbox, etc.
 *   - Section nodes: system:library, section:folders, section:smart_folders
 *   - Folder IDs: folder:{id}, parent defaults to section:folders
 *   - Smart folder IDs: smart:{id}, parent defaults to section:smart_folders
 *   - Smart folder meta: top-level keys (smart_folder_id, parent_id, predicate,
 *     local_predicate, has_effective_rules, has_local_rules, sort_field, sort_order)
 *   - Freshness values: "fresh", "stale", "rebuilding"
 */

import type { SidebarNodeDto, SidebarTreeResponse } from '../../shared/types/canonical';

// ── Section nodes (non-selectable structural containers) ─────────

const sectionLibrary: SidebarNodeDto = {
  id: 'system:library',
  kind: 'section',
  parent_id: null,
  name: 'Library',
  icon: null,
  sort_order: 0,
  count: null,
  freshness: 'exact',
  selectable: false,
  expanded_by_default: true,
};

const sectionFolders: SidebarNodeDto = {
  id: 'section:folders',
  kind: 'section',
  parent_id: null,
  name: 'Folders',
  sort_order: 10,
  count: null,
  freshness: 'exact',
  selectable: false,
  expanded_by_default: true,
};

const sectionSmartFolders: SidebarNodeDto = {
  id: 'section:smart_folders',
  kind: 'section',
  parent_id: null,
  name: 'Smart Folders',
  sort_order: 20,
  count: null,
  freshness: 'exact',
  selectable: false,
  expanded_by_default: true,
};

// ── System scope nodes (children of system:library) ──────────────

const systemActive: SidebarNodeDto = {
  id: 'system:active',
  kind: 'system',
  parent_id: 'system:library',
  name: 'All Active',
  icon: 'IconPhoto',
  sort_order: 1,
  count: 1247,
  freshness: 'exact',
  selectable: true,
};

const systemInbox: SidebarNodeDto = {
  id: 'system:inbox',
  kind: 'system',
  parent_id: 'system:library',
  name: 'Inbox',
  icon: 'IconInbox',
  sort_order: 2,
  count: 23,
  freshness: 'exact',
  selectable: true,
};

const systemUncategorized: SidebarNodeDto = {
  id: 'system:uncategorized',
  kind: 'system',
  parent_id: 'system:library',
  name: 'Uncategorized',
  icon: 'IconFolderQuestion',
  sort_order: 3,
  count: 89,
  freshness: 'exact',
  selectable: true,
};

const systemUntagged: SidebarNodeDto = {
  id: 'system:untagged',
  kind: 'system',
  parent_id: 'system:library',
  name: 'Untagged',
  icon: 'IconTagOff',
  sort_order: 4,
  count: 412,
  freshness: 'exact',
  selectable: true,
};

const systemRecentViewed: SidebarNodeDto = {
  id: 'system:recent_viewed',
  kind: 'system',
  parent_id: 'system:library',
  name: 'Recently Viewed',
  icon: 'IconEye',
  sort_order: 5,
  count: 0,
  freshness: 'exact',
  selectable: true,
};

const systemDuplicates: SidebarNodeDto = {
  id: 'system:duplicates',
  kind: 'system',
  parent_id: 'system:library',
  name: 'Duplicates',
  icon: 'IconCopy',
  sort_order: 6,
  count: 12,
  freshness: 'exact',
  selectable: true,
};

const systemTrash: SidebarNodeDto = {
  id: 'system:trash',
  kind: 'system',
  parent_id: 'system:library',
  name: 'Trash',
  icon: 'IconTrash',
  sort_order: 7,
  count: 5,
  freshness: 'exact',
  selectable: true,
};

// ── Folder nodes (children of section:folders) ───────────────────

const folderArtwork: SidebarNodeDto = {
  id: 'folder:1',
  kind: 'folder',
  parent_id: 'section:folders',
  name: 'Artwork',
  icon: 'palette',
  color: '#e57373',
  sort_order: 0,
  count: 342,
  freshness: 'exact',
  selectable: true,
  meta: { folder_id: 1, auto_tags: null },
};

// Nested folder under Artwork (parent is folder:1, not section:folders)
const folderCharacters: SidebarNodeDto = {
  id: 'folder:2',
  kind: 'folder',
  parent_id: 'folder:1',
  name: 'Characters',
  sort_order: 0,
  count: 156,
  freshness: 'exact',
  selectable: true,
  meta: { folder_id: 2, auto_tags: null },
};

// Deeply nested folder (3 levels)
const folderOriginal: SidebarNodeDto = {
  id: 'folder:3',
  kind: 'folder',
  parent_id: 'folder:2',
  name: 'Original Characters',
  sort_order: 0,
  count: 44,
  freshness: 'exact',
  selectable: true,
  meta: { folder_id: 3, auto_tags: null },
};

// Empty folder
const folderEmpty: SidebarNodeDto = {
  id: 'folder:4',
  kind: 'folder',
  parent_id: 'section:folders',
  name: 'Unsorted',
  sort_order: 1,
  count: 0,
  freshness: 'exact',
  selectable: true,
  meta: { folder_id: 4, auto_tags: null },
};

// Folder with auto_tags
const folderWatched: SidebarNodeDto = {
  id: 'folder:5',
  kind: 'folder',
  parent_id: 'section:folders',
  name: 'Downloads',
  icon: 'download',
  color: '#4fc3f7',
  sort_order: 2,
  count: 67,
  freshness: 'exact',
  selectable: true,
  meta: { folder_id: 5, auto_tags: 'downloaded' },
};

// ── Smart folder nodes (children of section:smart_folders) ───────

const smartHighRated: SidebarNodeDto = {
  id: 'smart:1',
  kind: 'smart_folder',
  parent_id: 'section:smart_folders',
  name: 'Highly Rated',
  icon: 'star',
  color: '#ffd54f',
  sort_order: 1,
  count: 89,
  freshness: 'exact',
  selectable: true,
  meta: {
    smart_folder_id: 1,
    parent_id: null,
    predicate: {
      groups: [{
        match_mode: 'all',
        negate: false,
        rules: [{ field: 'rating', op: 'gte', value: 4 }],
      }],
    },
    local_predicate: {
      groups: [{
        match_mode: 'all',
        negate: false,
        rules: [{ field: 'rating', op: 'gte', value: 4 }],
      }],
    },
    has_effective_rules: true,
    has_local_rules: true,
  },
};

const smartRecent: SidebarNodeDto = {
  id: 'smart:2',
  kind: 'smart_folder',
  parent_id: 'section:smart_folders',
  name: 'Recent Imports',
  sort_order: 2,
  count: 31,
  freshness: 'stale',
  selectable: true,
  meta: {
    smart_folder_id: 2,
    parent_id: null,
    predicate: {
      groups: [{
        match_mode: 'all',
        negate: false,
        rules: [{ field: 'date_added', op: 'gte', value: '2026-03-01' }],
      }],
    },
    local_predicate: {
      groups: [{
        match_mode: 'all',
        negate: false,
        rules: [{ field: 'date_added', op: 'gte', value: '2026-03-01' }],
      }],
    },
    has_effective_rules: true,
    has_local_rules: true,
    sort_field: 'date_added',
    sort_order: 'desc',
  },
};

// ── Assembled trees ──────────────────────────────────────────────

/** Standard sidebar tree matching the live backend contract. */
export const sidebarTreeStandard: SidebarTreeResponse = {
  nodes: [
    sectionLibrary,
    systemActive,
    systemInbox,
    systemUncategorized,
    systemUntagged,
    systemRecentViewed,
    systemDuplicates,
    systemTrash,
    sectionFolders,
    folderArtwork,
    folderCharacters,
    folderOriginal,
    folderEmpty,
    folderWatched,
    sectionSmartFolders,
    smartHighRated,
    smartRecent,
  ],
  tree_epoch: 42,
  generated_at: '2026-03-24T10:00:00Z',
};

/** Empty library — sections + system scopes only, all counts zero. */
export const sidebarTreeEmpty: SidebarTreeResponse = {
  nodes: [
    sectionLibrary,
    { ...systemActive, count: 0 },
    { ...systemInbox, count: 0 },
    { ...systemUncategorized, count: 0 },
    { ...systemUntagged, count: 0 },
    { ...systemRecentViewed, count: 0 },
    { ...systemDuplicates, count: 0 },
    { ...systemTrash, count: 0 },
    sectionFolders,
    sectionSmartFolders,
  ],
  tree_epoch: 1,
  generated_at: '2026-03-24T10:00:00Z',
};

/** Sidebar with stale counts (post-import, pre-compiler-rebuild). */
export const sidebarTreeStale: SidebarTreeResponse = {
  nodes: [
    sectionLibrary,
    { ...systemActive, freshness: 'stale' as const },
    { ...systemInbox, freshness: 'stale' as const },
    systemTrash,
    { ...systemUncategorized, freshness: 'rebuilding' as const },
    systemUntagged,
    { ...systemRecentViewed, count: 0 },
    systemDuplicates,
    sectionFolders,
    { ...folderArtwork, freshness: 'rebuilding' as const },
    folderCharacters,
    folderOriginal,
    folderEmpty,
    folderWatched,
    sectionSmartFolders,
    smartHighRated,
    { ...smartRecent, freshness: 'stale' as const },
  ],
  tree_epoch: 43,
  generated_at: '2026-03-24T10:01:00Z',
};

// ── Exported individual nodes for targeted tests ─────────────────

export const nodes = {
  sectionLibrary,
  sectionFolders,
  sectionSmartFolders,
  systemActive,
  systemInbox,
  systemUncategorized,
  systemUntagged,
  systemRecentViewed,
  systemDuplicates,
  systemTrash,
  folderArtwork,
  folderCharacters,
  folderOriginal,
  folderEmpty,
  folderWatched,
  smartHighRated,
  smartRecent,
};
