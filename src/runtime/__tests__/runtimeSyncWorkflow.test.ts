/**
 * PBI-516: Workflow tests for runtime state change → refresh target planning.
 *
 * Verifies that multi-step state-change sequences accumulate the correct
 * set of refresh targets, and that batch operations scale correctly.
 */

import { describe, it, expect } from 'vitest';
import { planRefreshTargets, refreshTargetMatchesGridScope } from '../stateChanges/planRefreshTargets';
import type { StateChangedEvent, StateChanges, ResourceKey } from '../../shared/types/backendState';


// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeChanges(overrides: Partial<StateChanges> = {}): StateChanges {
  return { domains: [], ...overrides };
}

function makeEvent(
  changes: StateChanges,
  extras: Partial<Pick<StateChangedEvent, 'sidebar_counts'>> = {},
  seq = 1,
): StateChangedEvent {
  return {
    seq,
    ts: '2026-01-01T00:00:00Z',
    origin: 'test',
    changes,
    ...extras,
  };
}

function keys(event: StateChangedEvent): Set<ResourceKey> {
  return planRefreshTargets(event);
}

/** Accumulate refresh targets across a sequence of state-changed events. */
function accumulateRefreshTargets(events: StateChangedEvent[]): Set<ResourceKey> {
  const all = new Set<ResourceKey>();
  for (const event of events) {
    for (const k of planRefreshTargets(event)) {
      all.add(k);
    }
  }
  return all;
}

// ---------------------------------------------------------------------------
// Multi-step state-change workflows
// ---------------------------------------------------------------------------

describe('multi-step state-change workflows', () => {
  it('full lifecycle: inbox → active → tag → folder produces correct cumulative refresh targets', () => {
    const events: StateChangedEvent[] = [
      // Step 1: status_changed (inbox → active)
      makeEvent(makeChanges({ status_changed: true }), {}, 1),
      // Step 2: tags_changed on specific files
      makeEvent(makeChanges({ tags_changed: true, file_hashes: ['abc'] }), {}, 2),
      // Step 3: folder_membership_changed
      makeEvent(makeChanges({ folder_membership_changed: [5] }), {}, 3),
    ];

    const targets = accumulateRefreshTargets(events);

    // From status_changed
    expect(targets).toContain('sidebar/tree');
    expect(targets).toContain('grid/system:all');
    expect(targets).toContain('grid/system:inbox');
    expect(targets).toContain('grid/system:trash');

    // From tags_changed
    expect(targets).toContain('selection/current');
    expect(targets).toContain('metadata/hash:abc');
    expect(targets).toContain('grid/system:untagged');

    // From folder_membership_changed
    expect(targets).toContain('grid/folder:5');
    expect(targets).toContain('grid/system:uncategorized');
  });

  it('batch tag change on N files produces all N metadata keys', () => {
    const hashes = Array.from({ length: 20 }, (_, i) => `hash_${i}`);
    const receipt = makeEvent(makeChanges({
      tags_changed: true,
      file_hashes: hashes,
    }));

    const targets = keys(receipt);

    for (const h of hashes) {
      expect(targets).toContain(`metadata/hash:${h}` as ResourceKey);
    }
    expect(targets).toContain('selection/current');
    expect(targets).toContain('grid/system:untagged');
  });

  it('smart folder predicate change targets a specific smart scope', () => {
    const receipt = makeEvent(makeChanges({
      smart_folder_ids: [42],
    }));

    const targets = keys(receipt);
    expect(targets).toContain('grid/smart:42');
    expect(targets).toContain('selection/current');

    // Should match any active smart scope
    expect(refreshTargetMatchesGridScope('grid/smart:42', 'smart:42')).toBe(true);
    expect(refreshTargetMatchesGridScope('grid/smart:42', 'smart:99')).toBe(false);
  });

  it('sidebar_counts in receipt produces sidebar/counts key', () => {
    const receipt = makeEvent(
      makeChanges({ status_changed: true }),
      { sidebar_counts: { all_active: 50, inbox: 3, trash: 1 } },
    );

    const targets = keys(receipt);
    expect(targets).toContain('sidebar/counts');
    // Also from status_changed
    expect(targets).toContain('sidebar/tree');
    expect(targets).toContain('grid/system:all');
  });

  it('collection delete targets folder and system scopes', () => {
    const receipt = makeEvent(makeChanges({
      folder_membership_changed: [10],
      status_changed: true,
      extra_grid_scopes: ['collection:42'],
    }));

    const targets = keys(receipt);
    expect(targets).toContain('grid/collection:42');
    expect(targets).toContain('grid/folder:10');
    expect(targets).toContain('grid/system:all');
    expect(targets).toContain('sidebar/tree');
  });

  it('compiler_batch_done + tag_structure_changed merges cleanly', () => {
    const receipt = makeEvent(makeChanges({
      compiler_batch_done: true,
      tag_structure_changed: true,
    }));

    const targets = keys(receipt);
    expect(targets).toContain('sidebar/tree');
    expect(targets).toContain('grid/system:all');
    expect(targets).toContain('grid/smart:all');
    expect(targets).toContain('selection/current');
  });
});
