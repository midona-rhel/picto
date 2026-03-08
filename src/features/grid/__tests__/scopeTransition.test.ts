/**
 * PBI-048 / PBI-153: Regression tests for scope-transition state machine.
 *
 * Validates that the deferred-replace pattern prevents premature context swaps
 * during fade-out transitions. The grid uses a TransitionStage enum:
 *   1. 'fading_out' — fade-out starts, front model frozen
 *   2. At midpoint (opacity = 0), scope/context/empty-state text is swapped
 *   3. 'fading_in' — fade-in starts with new scope content
 *   4. 'idle' — transition complete
 *
 * These tests verify the state machine logic, not the visual rendering.
 */

import { describe, it, expect } from 'vitest';

// Simulate the scope transition state machine used in ImageGrid.
// This models the deferred-replace pattern without React rendering.

type TransitionStage = 'idle' | 'fading_out' | 'fading_in';
type GridEmptyContext = 'default' | 'inbox' | 'untagged' | 'folder' | 'smart-folder';

interface ScopeTransitionState {
  transitionStage: TransitionStage;
  displayEmptyContext: GridEmptyContext;
  pendingContext: GridEmptyContext | null;
}

function createInitialState(context: GridEmptyContext): ScopeTransitionState {
  return {
    transitionStage: 'idle',
    displayEmptyContext: context,
    pendingContext: null,
  };
}

// Simulates the scope change trigger (equivalent to the folderId/smartFolder useEffect)
function triggerScopeChange(state: ScopeTransitionState, newContext: GridEmptyContext): ScopeTransitionState {
  return {
    ...state,
    transitionStage: 'fading_out', // Start fade-out, front model frozen
    pendingContext: newContext,
    // CRITICAL: displayEmptyContext does NOT change yet
  };
}

// Simulates the midpoint callback (opacity = 0, safe to swap)
function commitScopeChange(state: ScopeTransitionState): ScopeTransitionState {
  if (!state.pendingContext) return state;
  return {
    ...state,
    displayEmptyContext: state.pendingContext, // NOW swap context
    pendingContext: null,
    transitionStage: 'fading_in', // Start fade-in with new content
  };
}

describe('scope transition state machine', () => {
  it('does not swap context before midpoint commit', () => {
    const state = createInitialState('default');
    const afterTrigger = triggerScopeChange(state, 'folder');

    // Context should still be 'default' — NOT 'folder'
    expect(afterTrigger.displayEmptyContext).toBe('default');
    expect(afterTrigger.transitionStage).toBe('fading_out');
    expect(afterTrigger.pendingContext).toBe('folder');
  });

  it('swaps context only at midpoint commit', () => {
    const state = createInitialState('default');
    const afterTrigger = triggerScopeChange(state, 'folder');
    const afterCommit = commitScopeChange(afterTrigger);

    expect(afterCommit.displayEmptyContext).toBe('folder');
    expect(afterCommit.transitionStage).toBe('fading_in');
    expect(afterCommit.pendingContext).toBeNull();
  });

  it('folder → smart-folder transition preserves old context during fade', () => {
    const state = createInitialState('folder');
    const fading = triggerScopeChange(state, 'smart-folder');

    expect(fading.displayEmptyContext).toBe('folder'); // still old
    const committed = commitScopeChange(fading);
    expect(committed.displayEmptyContext).toBe('smart-folder'); // now new
  });

  it('folder → default transition preserves old context during fade', () => {
    const state = createInitialState('folder');
    const fading = triggerScopeChange(state, 'default');

    expect(fading.displayEmptyContext).toBe('folder');
    const committed = commitScopeChange(fading);
    expect(committed.displayEmptyContext).toBe('default');
  });

  it('inbox → folder transition', () => {
    const state = createInitialState('inbox');
    const fading = triggerScopeChange(state, 'folder');

    expect(fading.displayEmptyContext).toBe('inbox');
    const committed = commitScopeChange(fading);
    expect(committed.displayEmptyContext).toBe('folder');
  });

  it('no-op commit when no pending context', () => {
    const state = createInitialState('default');
    const committed = commitScopeChange(state);

    expect(committed.displayEmptyContext).toBe('default');
    expect(committed).toBe(state); // same reference
  });

  it('rapid transitions keep only the latest pending context', () => {
    const state = createInitialState('default');
    // First transition starts
    const mid1 = triggerScopeChange(state, 'folder');
    // Before commit, another transition comes in
    const mid2 = triggerScopeChange(mid1, 'smart-folder');

    // Context should still be the original
    expect(mid2.displayEmptyContext).toBe('default');
    // But pending is the latest
    expect(mid2.pendingContext).toBe('smart-folder');

    const committed = commitScopeChange(mid2);
    expect(committed.displayEmptyContext).toBe('smart-folder');
  });
});
