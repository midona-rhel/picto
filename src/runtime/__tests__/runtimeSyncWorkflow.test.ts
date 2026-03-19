/**
 * PBI-516: Workflow tests for runtime mutation → resource staleness.
 *
 * Verifies that multi-step mutation sequences accumulate the correct
 * set of stale resources, and that batch operations scale correctly.
 */

import { describe, it, expect } from 'vitest';
import { deriveStaleResources, gridResourceMatchesScope } from '../resourceInvalidator';
import type { MutationReceipt, MutationFacts, ResourceKey } from '../../shared/types/generated/runtime-contract';
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
  seq = 1,
): MutationReceipt {
  return {
    seq,
    ts: '2026-01-01T00:00:00Z',
    origin_command: 'test',
    facts,
    ...extras,
  };
}

function keys(receipt: MutationReceipt): Set<ResourceKey> {
  return deriveStaleResources(receipt);
}

/** Accumulate stale resources across a sequence of receipts. */
function accumulateStale(receipts: MutationReceipt[]): Set<ResourceKey> {
  const all = new Set<ResourceKey>();
  for (const r of receipts) {
    for (const k of deriveStaleResources(r)) {
      all.add(k);
    }
  }
  return all;
}

// ---------------------------------------------------------------------------
// Multi-step mutation workflows
// ---------------------------------------------------------------------------

describe('multi-step mutation workflows', () => {
  it('full lifecycle: inbox → active → tag → folder produces correct cumulative staleness', () => {
    const receipts: MutationReceipt[] = [
      // Step 1: status_changed (inbox → active)
      makeReceipt(makeFacts({ status_changed: true }), {}, 1),
      // Step 2: tags_changed on specific files
      makeReceipt(makeFacts({ tags_changed: true, file_hashes: ['abc'] }), {}, 2),
      // Step 3: folder_membership_changed
      makeReceipt(makeFacts({ folder_membership_changed: [5] }), {}, 3),
    ];

    const stale = accumulateStale(receipts);

    // From status_changed
    expect(stale).toContain('sidebar/tree');
    expect(stale).toContain('grid/system:all');
    expect(stale).toContain('grid/system:inbox');
    expect(stale).toContain('grid/system:trash');

    // From tags_changed
    expect(stale).toContain('selection/current');
    expect(stale).toContain('metadata/hash:abc');
    expect(stale).toContain('grid/system:untagged');

    // From folder_membership_changed
    expect(stale).toContain('grid/folder:5');
    expect(stale).toContain('grid/system:uncategorized');
  });

  it('batch tag change on N files produces all N metadata keys', () => {
    const hashes = Array.from({ length: 20 }, (_, i) => `hash_${i}`);
    const receipt = makeReceipt(makeFacts({
      tags_changed: true,
      file_hashes: hashes,
    }));

    const stale = keys(receipt);

    for (const h of hashes) {
      expect(stale).toContain(`metadata/hash:${h}` as ResourceKey);
    }
    expect(stale).toContain('selection/current');
    expect(stale).toContain('grid/system:untagged');
  });

  it('smart folder predicate change invalidates specific smart scope', () => {
    const receipt = makeReceipt(makeFacts({
      smart_folder_ids: [42],
    }));

    const stale = keys(receipt);
    expect(stale).toContain('grid/smart:42');
    expect(stale).toContain('selection/current');

    // Should match any active smart scope
    expect(gridResourceMatchesScope('grid/smart:42', 'smart:42')).toBe(true);
    expect(gridResourceMatchesScope('grid/smart:42', 'smart:99')).toBe(false);
  });

  it('sidebar_counts in receipt produces sidebar/counts key', () => {
    const receipt = makeReceipt(
      makeFacts({ status_changed: true }),
      { sidebar_counts: { all_active: 50, inbox: 3, trash: 1 } },
    );

    const stale = keys(receipt);
    expect(stale).toContain('sidebar/counts');
    // Also from status_changed
    expect(stale).toContain('sidebar/tree');
    expect(stale).toContain('grid/system:all');
  });

  it('collection delete invalidates folder and system scopes', () => {
    const receipt = makeReceipt(makeFacts({
      folder_membership_changed: [10],
      status_changed: true,
      extra_grid_scopes: ['collection:42'],
    }));

    const stale = keys(receipt);
    expect(stale).toContain('grid/collection:42');
    expect(stale).toContain('grid/folder:10');
    expect(stale).toContain('grid/system:all');
    expect(stale).toContain('sidebar/tree');
  });

  it('compiler_batch_done + tag_structure_changed merges cleanly', () => {
    const receipt = makeReceipt(makeFacts({
      compiler_batch_done: true,
      tag_structure_changed: true,
    }));

    const stale = keys(receipt);
    expect(stale).toContain('sidebar/tree');
    expect(stale).toContain('grid/system:all');
    expect(stale).toContain('grid/smart:all');
    expect(stale).toContain('selection/current');
  });
});
