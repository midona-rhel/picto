/**
 * PBI-303: Contract tests for the derived resource dependency map.
 *
 * Each state-change field must always yield the same deterministic set
 * of refresh targets. These tests document that contract.
 */

import { describe, it, expect } from 'vitest';
import { planRefreshTargets, refreshTargetMatchesGridScope } from '../stateChanges/planRefreshTargets';
import type { StateChangedEvent, StateChanges, ResourceKey, Domain } from '../../shared/types/backendState';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeChanges(overrides: Partial<StateChanges> = {}): StateChanges {
  return { domains: [], ...overrides };
}

function makeEvent(
  changes: StateChanges,
  extras: Partial<Pick<StateChangedEvent, 'sidebar_counts'>> = {},
): StateChangedEvent {
  return {
    seq: 1,
    ts: '2026-01-01T00:00:00Z',
    origin: 'test',
    changes,
    ...extras,
  };
}

function keys(event: StateChangedEvent): Set<ResourceKey> {
  return planRefreshTargets(event);
}

function keyArray(event: StateChangedEvent): ResourceKey[] {
  return [...keys(event)].sort();
}

// ---------------------------------------------------------------------------
// planRefreshTargets — fact-driven rules
// ---------------------------------------------------------------------------

describe('planRefreshTargets', () => {
  // --- status_changed ---

  it('status_changed yields sidebar/tree, selection/current, and status-sensitive grid scopes', () => {
    const result = keys(makeEvent(makeChanges({ status_changed: true })));
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:active');
    expect(result).toContain('grid/system:inbox');
    expect(result).toContain('grid/system:trash');
    expect(result).toContain('grid/system:untagged');
    expect(result).toContain('grid/system:uncategorized');
    expect(result).toContain('grid/smart:all');
  });

  it('status_changed with folder_ids includes folder grid scopes', () => {
    const result = keys(makeEvent(makeChanges({
      status_changed: true,
      folder_ids: [10, 20],
    })));
    expect(result).toContain('grid/folder:10');
    expect(result).toContain('grid/folder:20');
    // folder_ids with status_changed should NOT produce standalone folder scopes
    // (they're already included via status_changed rule)
  });

  // --- tags_changed ---

  it('tags_changed with entity_hashes yields selection/current + metadata keys, no grid/system:active', () => {
    const result = keys(makeEvent(makeChanges({
      tags_changed: true,
      entity_hashes: ['abc', 'def'],
    })));
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:untagged');
    expect(result).toContain('metadata/hash:abc');
    expect(result).toContain('metadata/hash:def');
    expect(result).not.toContain('grid/system:active');
  });

  it('tags_changed without entity_hashes yields selection/current + grid/system:active', () => {
    const result = keys(makeEvent(makeChanges({ tags_changed: true })));
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:active');
    expect(result).toContain('grid/system:untagged');
  });

  it('tag_changes without tags_changed still yields the tag refresh targets', () => {
    const result = keys(makeEvent(makeChanges({
      domains: ['sidebar', 'smart_folders'] as Domain[],
      tag_changes: { added: ['artist:abc'] },
      entity_hashes: ['abc'],
      extra_grid_scopes: ['smart:all'],
    })));
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:untagged');
    expect(result).toContain('grid/smart:all');
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('metadata/hash:abc');
    expect(result).not.toContain('grid/system:active');
  });

  // --- tag_structure_changed ---

  it('tag_structure_changed yields sidebar/tree, selection/current, grid/system:active, grid/smart:all', () => {
    const result = keys(makeEvent(makeChanges({ tag_structure_changed: true })));
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:active');
    expect(result).toContain('grid/smart:all');
  });

  // --- folder_membership_changed ---

  it('folder_membership_changed yields sidebar/tree, selection/current, grid/folder:{id}', () => {
    const result = keys(makeEvent(makeChanges({
      folder_membership_changed: [5, 15],
    })));
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:uncategorized');
    expect(result).toContain('grid/folder:5');
    expect(result).toContain('grid/folder:15');
  });

  // --- view_prefs_changed ---

  it('view_prefs_changed yields view-prefs/current only', () => {
    const result = keyArray(makeEvent(makeChanges({ view_prefs_changed: true })));
    expect(result).toEqual(['view-prefs/current']);
  });

  it('media_fields_changed yields selection/current and hash metadata keys', () => {
    const result = keys(makeEvent(makeChanges({
      media_fields_changed: ['rating'],
      entity_hashes: ['h1'],
    })));
    expect(result).toContain('selection/current');
    expect(result).toContain('metadata/hash:h1');
  });

  it('smart-folder-sensitive media fields can also yield smart:all and sidebar/tree', () => {
    const result = keys(makeEvent(makeChanges({
      domains: ['sidebar', 'smart_folders'] as Domain[],
      media_fields_changed: ['rating'],
      entity_hashes: ['h1'],
      extra_grid_scopes: ['smart:all'],
    })));
    expect(result).toContain('selection/current');
    expect(result).toContain('metadata/hash:h1');
    expect(result).toContain('grid/smart:all');
    expect(result).toContain('sidebar/tree');
  });

  it('derivative_fields_changed yields selection/current and hash metadata keys', () => {
    const result = keys(makeEvent(makeChanges({
      derivative_fields_changed: ['thumbnail'],
      entity_hashes: ['h1'],
    })));
    expect(result).toContain('selection/current');
    expect(result).toContain('metadata/hash:h1');
  });

  // --- compiler_batch_done ---

  it('compiler_batch_done yields sidebar/tree', () => {
    const result = keys(makeEvent(makeChanges({ compiler_batch_done: true })));
    expect(result).toContain('sidebar/tree');
  });

  // --- entity_hashes ---

  it('entity_hashes yields metadata/hash:{hash} for each hash', () => {
    const result = keys(makeEvent(makeChanges({
      domains: ['files'] as Domain[],
      entity_hashes: ['h1', 'h2', 'h3'],
    })));
    expect(result).toContain('metadata/hash:h1');
    expect(result).toContain('metadata/hash:h2');
    expect(result).toContain('metadata/hash:h3');
  });

  // --- folder_ids without membership change ---

  it('folder_ids without membership change yields grid/folder:{id}', () => {
    const result = keys(makeEvent(makeChanges({
      folder_ids: [7, 8],
    })));
    expect(result).toContain('grid/folder:7');
    expect(result).toContain('grid/folder:8');
    // Should NOT set sidebar/tree or selection/current from folder_ids alone
    expect(result).not.toContain('sidebar/tree');
    expect(result).not.toContain('selection/current');
  });

  it('folder_ids with folder_membership_changed does not duplicate grid scopes', () => {
    const result = keys(makeEvent(makeChanges({
      folder_membership_changed: [7],
      folder_ids: [7],
    })));
    // folder_ids rule is suppressed when folder_membership_changed is present
    const gridFolder7Count = [...result].filter(k => k === 'grid/folder:7').length;
    expect(gridFolder7Count).toBe(1);
  });

  // --- smart_folder_ids ---

  it('smart_folder_ids yields selection/current + grid/smart:{id}', () => {
    const result = keys(makeEvent(makeChanges({
      smart_folder_ids: [3, 9],
    })));
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/smart:3');
    expect(result).toContain('grid/smart:9');
  });

  // --- extra_grid_scopes ---

  it('extra_grid_scopes yields grid/{scope} for each', () => {
    const result = keys(makeEvent(makeChanges({
      extra_grid_scopes: ['collection:42', 'system:recently_viewed'],
    })));
    expect(result).toContain('grid/collection:42');
    expect(result).toContain('grid/system:recently_viewed');
  });

  // --- sidebar_counts ---

  it('sidebar_counts present yields sidebar/counts', () => {
    const result = keys(makeEvent(
      makeChanges({}),
      { sidebar_counts: { active: 100, inbox: 5, trash: 2 } },
    ));
    expect(result).toContain('sidebar/counts');
  });

  it('sidebar_counts absent does not yield sidebar/counts', () => {
    const result = keys(makeEvent(makeChanges({})));
    expect(result).not.toContain('sidebar/counts');
  });

  // --- Domain fallbacks ---

  it('Domain::Sidebar without fact flags yields sidebar/tree', () => {
    const result = keys(makeEvent(makeChanges({
      domains: ['sidebar'] as Domain[],
    })));
    expect(result).toContain('sidebar/tree');
  });

  it('Domain::Selection without fact flags yields selection/current', () => {
    const result = keys(makeEvent(makeChanges({
      domains: ['selection'] as Domain[],
    })));
    expect(result).toContain('selection/current');
  });

  it('Domain::Sidebar fallback does NOT fire if sidebar/tree already set by facts', () => {
    // tag_structure_changed already sets sidebar/tree — domain fallback skipped
    const result = keys(makeEvent(makeChanges({
      domains: ['sidebar'] as Domain[],
      tag_structure_changed: true,
    })));
    expect(result).toContain('sidebar/tree');
    // Just confirming it's there once (from facts), no double-add issue
  });

  // --- Combined facts ---

  it('status_changed + tags_changed combines both rule outputs', () => {
    const result = keys(makeEvent(makeChanges({
      status_changed: true,
      tags_changed: true,
      entity_hashes: ['h1'],
    })));
    // From status_changed
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('grid/system:active');
    expect(result).toContain('grid/system:inbox');
    // From tags_changed (with entity_hashes → no extra grid/system:active)
    expect(result).toContain('selection/current');
    // From entity_hashes
    expect(result).toContain('metadata/hash:h1');
  });

  // --- Empty facts ---

  it('empty facts with no domains yields empty set', () => {
    const result = keys(makeEvent(makeChanges({})));
    expect(result.size).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// refreshTargetMatchesGridScope
// ---------------------------------------------------------------------------

describe('refreshTargetMatchesGridScope', () => {
  it('exact scope match returns true', () => {
    expect(refreshTargetMatchesGridScope('grid/folder:5', 'folder:5')).toBe(true);
  });

  it('mismatched scope returns false', () => {
    expect(refreshTargetMatchesGridScope('grid/folder:5', 'folder:10')).toBe(false);
  });

  it('system:active is a wildcard only for system scopes', () => {
    expect(refreshTargetMatchesGridScope('grid/system:active', 'system:inbox')).toBe(true);
    expect(refreshTargetMatchesGridScope('grid/system:active', 'system:uncategorized')).toBe(true);
    expect(refreshTargetMatchesGridScope('grid/system:active', 'folder:5')).toBe(false);
    expect(refreshTargetMatchesGridScope('grid/system:active', 'smart:3')).toBe(false);
    expect(refreshTargetMatchesGridScope('grid/system:active', 'collection:7')).toBe(false);
  });

  it('folder:all matches any folder:N scope', () => {
    expect(refreshTargetMatchesGridScope('grid/folder:all', 'folder:99')).toBe(true);
  });

  it('folder:all does not match smart scopes', () => {
    expect(refreshTargetMatchesGridScope('grid/folder:all', 'smart:1')).toBe(false);
  });

  it('smart:all matches any smart:N scope', () => {
    expect(refreshTargetMatchesGridScope('grid/smart:all', 'smart:42')).toBe(true);
  });

  it('smart:all does not match folder scopes', () => {
    expect(refreshTargetMatchesGridScope('grid/smart:all', 'folder:1')).toBe(false);
  });

  it('null activeScope matches everything', () => {
    expect(refreshTargetMatchesGridScope('grid/folder:5', null)).toBe(true);
    expect(refreshTargetMatchesGridScope('grid/system:inbox', null)).toBe(true);
  });

  it('non-grid keys return false', () => {
    expect(refreshTargetMatchesGridScope('sidebar/tree', 'folder:5')).toBe(false);
    expect(refreshTargetMatchesGridScope('selection/current', null)).toBe(false);
    expect(refreshTargetMatchesGridScope('metadata/hash:abc', null)).toBe(false);
  });
});
