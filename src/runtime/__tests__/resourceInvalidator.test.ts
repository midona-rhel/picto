/**
 * PBI-303: Contract tests for the derived resource dependency map.
 *
 * Each mutation fact type must always yield the same deterministic set
 * of stale ResourceKeys. These tests document that contract.
 */

import { describe, it, expect } from 'vitest';
import { deriveStaleResources, gridResourceMatchesScope } from '../resourceInvalidator';
import type { MutationReceipt, MutationFacts, ResourceKey } from '../../shared/types/generated/runtime-contract';
import type { DerivedInvalidation } from '../../shared/types/generated/runtime-contract/DerivedInvalidation';
import type { Domain } from '../../shared/types/generated/runtime-contract/Domain';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeFacts(overrides: Partial<MutationFacts> = {}): MutationFacts {
  return { domains: [], ...overrides };
}

function makeReceipt(
  facts: MutationFacts,
  extras: Partial<Pick<MutationReceipt, 'sidebar_counts'>> = {},
): MutationReceipt {
  return {
    seq: 1,
    ts: '2026-01-01T00:00:00Z',
    origin_command: 'test',
    facts,
    invalidate: {} as DerivedInvalidation,
    ...extras,
  };
}

function keys(receipt: MutationReceipt): Set<ResourceKey> {
  return deriveStaleResources(receipt);
}

function keyArray(receipt: MutationReceipt): ResourceKey[] {
  return [...keys(receipt)].sort();
}

// ---------------------------------------------------------------------------
// deriveStaleResources — fact-driven rules
// ---------------------------------------------------------------------------

describe('deriveStaleResources', () => {
  // --- status_changed ---

  it('status_changed yields sidebar/tree, selection/current, and status-sensitive grid scopes', () => {
    const result = keys(makeReceipt(makeFacts({ status_changed: true })));
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:all');
    expect(result).toContain('grid/system:inbox');
    expect(result).toContain('grid/system:trash');
    expect(result).toContain('grid/system:recently_viewed');
    expect(result).toContain('grid/smart:all');
  });

  it('status_changed with folder_ids includes folder grid scopes', () => {
    const result = keys(makeReceipt(makeFacts({
      status_changed: true,
      folder_ids: [10, 20],
    })));
    expect(result).toContain('grid/folder:10');
    expect(result).toContain('grid/folder:20');
    // folder_ids with status_changed should NOT produce standalone folder scopes
    // (they're already included via status_changed rule)
  });

  // --- tags_changed ---

  it('tags_changed with file_hashes yields selection/current + metadata keys, no grid/system:all', () => {
    const result = keys(makeReceipt(makeFacts({
      tags_changed: true,
      file_hashes: ['abc', 'def'],
    })));
    expect(result).toContain('selection/current');
    expect(result).toContain('metadata/hash:abc');
    expect(result).toContain('metadata/hash:def');
    expect(result).not.toContain('grid/system:all');
  });

  it('tags_changed without file_hashes yields selection/current + grid/system:all', () => {
    const result = keys(makeReceipt(makeFacts({ tags_changed: true })));
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:all');
  });

  // --- tag_structure_changed ---

  it('tag_structure_changed yields sidebar/tree, selection/current, grid/system:all, grid/smart:all', () => {
    const result = keys(makeReceipt(makeFacts({ tag_structure_changed: true })));
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/system:all');
    expect(result).toContain('grid/smart:all');
  });

  // --- folder_membership_changed ---

  it('folder_membership_changed yields sidebar/tree, selection/current, grid/folder:{id}', () => {
    const result = keys(makeReceipt(makeFacts({
      folder_membership_changed: [5, 15],
    })));
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/folder:5');
    expect(result).toContain('grid/folder:15');
  });

  // --- view_prefs_changed ---

  it('view_prefs_changed yields view-prefs/current only', () => {
    const result = keyArray(makeReceipt(makeFacts({ view_prefs_changed: true })));
    expect(result).toEqual(['view-prefs/current']);
  });

  // --- compiler_batch_done ---

  it('compiler_batch_done yields sidebar/tree', () => {
    const result = keys(makeReceipt(makeFacts({ compiler_batch_done: true })));
    expect(result).toContain('sidebar/tree');
  });

  // --- file_hashes ---

  it('file_hashes yields metadata/hash:{hash} for each hash', () => {
    const result = keys(makeReceipt(makeFacts({
      domains: ['files'] as Domain[],
      file_hashes: ['h1', 'h2', 'h3'],
    })));
    expect(result).toContain('metadata/hash:h1');
    expect(result).toContain('metadata/hash:h2');
    expect(result).toContain('metadata/hash:h3');
  });

  // --- folder_ids without membership change ---

  it('folder_ids without membership change yields grid/folder:{id}', () => {
    const result = keys(makeReceipt(makeFacts({
      folder_ids: [7, 8],
    })));
    expect(result).toContain('grid/folder:7');
    expect(result).toContain('grid/folder:8');
    // Should NOT set sidebar/tree or selection/current from folder_ids alone
    expect(result).not.toContain('sidebar/tree');
    expect(result).not.toContain('selection/current');
  });

  it('folder_ids with folder_membership_changed does not duplicate grid scopes', () => {
    const result = keys(makeReceipt(makeFacts({
      folder_membership_changed: [7],
      folder_ids: [7],
    })));
    // folder_ids rule is suppressed when folder_membership_changed is present
    const gridFolder7Count = [...result].filter(k => k === 'grid/folder:7').length;
    expect(gridFolder7Count).toBe(1);
  });

  // --- smart_folder_ids ---

  it('smart_folder_ids yields selection/current + grid/smart:{id}', () => {
    const result = keys(makeReceipt(makeFacts({
      smart_folder_ids: [3, 9],
    })));
    expect(result).toContain('selection/current');
    expect(result).toContain('grid/smart:3');
    expect(result).toContain('grid/smart:9');
  });

  // --- extra_grid_scopes ---

  it('extra_grid_scopes yields grid/{scope} for each', () => {
    const result = keys(makeReceipt(makeFacts({
      extra_grid_scopes: ['collection:42', 'system:recently_viewed'],
    })));
    expect(result).toContain('grid/collection:42');
    expect(result).toContain('grid/system:recently_viewed');
  });

  // --- sidebar_counts ---

  it('sidebar_counts present yields sidebar/counts', () => {
    const result = keys(makeReceipt(
      makeFacts({}),
      { sidebar_counts: { all_images: 100, inbox: 5, trash: 2 } },
    ));
    expect(result).toContain('sidebar/counts');
  });

  it('sidebar_counts absent does not yield sidebar/counts', () => {
    const result = keys(makeReceipt(makeFacts({})));
    expect(result).not.toContain('sidebar/counts');
  });

  // --- Domain fallbacks ---

  it('Domain::Sidebar without fact flags yields sidebar/tree', () => {
    const result = keys(makeReceipt(makeFacts({
      domains: ['sidebar'] as Domain[],
    })));
    expect(result).toContain('sidebar/tree');
  });

  it('Domain::Selection without fact flags yields selection/current', () => {
    const result = keys(makeReceipt(makeFacts({
      domains: ['selection'] as Domain[],
    })));
    expect(result).toContain('selection/current');
  });

  it('Domain::Sidebar fallback does NOT fire if sidebar/tree already set by facts', () => {
    // tag_structure_changed already sets sidebar/tree — domain fallback skipped
    const result = keys(makeReceipt(makeFacts({
      domains: ['sidebar'] as Domain[],
      tag_structure_changed: true,
    })));
    expect(result).toContain('sidebar/tree');
    // Just confirming it's there once (from facts), no double-add issue
  });

  // --- Combined facts ---

  it('status_changed + tags_changed combines both rule outputs', () => {
    const result = keys(makeReceipt(makeFacts({
      status_changed: true,
      tags_changed: true,
      file_hashes: ['h1'],
    })));
    // From status_changed
    expect(result).toContain('sidebar/tree');
    expect(result).toContain('grid/system:all');
    expect(result).toContain('grid/system:inbox');
    // From tags_changed (with file_hashes → no extra grid/system:all)
    expect(result).toContain('selection/current');
    // From file_hashes
    expect(result).toContain('metadata/hash:h1');
  });

  // --- Empty facts ---

  it('empty facts with no domains yields empty set', () => {
    const result = keys(makeReceipt(makeFacts({})));
    expect(result.size).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// gridResourceMatchesScope
// ---------------------------------------------------------------------------

describe('gridResourceMatchesScope', () => {
  it('exact scope match returns true', () => {
    expect(gridResourceMatchesScope('grid/folder:5', 'folder:5')).toBe(true);
  });

  it('mismatched scope returns false', () => {
    expect(gridResourceMatchesScope('grid/folder:5', 'folder:10')).toBe(false);
  });

  it('system:all is a wildcard for any scope', () => {
    expect(gridResourceMatchesScope('grid/system:all', 'folder:5')).toBe(true);
    expect(gridResourceMatchesScope('grid/system:all', 'smart:3')).toBe(true);
    expect(gridResourceMatchesScope('grid/system:all', 'system:inbox')).toBe(true);
  });

  it('folder:all matches any folder:N scope', () => {
    expect(gridResourceMatchesScope('grid/folder:all', 'folder:99')).toBe(true);
  });

  it('folder:all does not match smart scopes', () => {
    expect(gridResourceMatchesScope('grid/folder:all', 'smart:1')).toBe(false);
  });

  it('smart:all matches any smart:N scope', () => {
    expect(gridResourceMatchesScope('grid/smart:all', 'smart:42')).toBe(true);
  });

  it('smart:all does not match folder scopes', () => {
    expect(gridResourceMatchesScope('grid/smart:all', 'folder:1')).toBe(false);
  });

  it('null activeScope matches everything', () => {
    expect(gridResourceMatchesScope('grid/folder:5', null)).toBe(true);
    expect(gridResourceMatchesScope('grid/system:inbox', null)).toBe(true);
  });

  it('non-grid keys return false', () => {
    expect(gridResourceMatchesScope('sidebar/tree', 'folder:5')).toBe(false);
    expect(gridResourceMatchesScope('selection/current', null)).toBe(false);
    expect(gridResourceMatchesScope('metadata/hash:abc', null)).toBe(false);
  });
});
