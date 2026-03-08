// ---------------------------------------------------------------------------
// Transition stage state machine
// ---------------------------------------------------------------------------
//
// Replaces the ad-hoc visible/fadePhase/gridTransitionLock triple with a
// single enum that drives opacity, CSS transition, and freeze semantics.
//
// Flow:  idle → fading_out → fading_in → idle
//

export type TransitionStage = 'idle' | 'fading_out' | 'fading_in';

// ---------------------------------------------------------------------------
// Timing constants — single source of truth for all transition durations
// ---------------------------------------------------------------------------

/** CSS transition duration for grid opacity fades (ms). */
export const FADE_DURATION_MS = 120;

/** Minimum wait before committing a transition — FADE_DURATION_MS + safety buffer (ms). */
export const FADE_SETTLE_MS = 130;

/** Debounce window for coalescing multi-step scope navigation state changes (ms). */
export const SCOPE_COALESCE_MS = 32;

// ---------------------------------------------------------------------------
// Selectors
// ---------------------------------------------------------------------------

/** CSS opacity value for the grid container. */
export function transitionOpacity(stage: TransitionStage): number {
  return stage === 'fading_out' ? 0 : 1;
}

/** CSS transition property for the grid container. */
export function transitionCss(stage: TransitionStage): string {
  return stage === 'idle' ? 'none' : `opacity ${FADE_DURATION_MS}ms ease`;
}

/**
 * Whether the front model is frozen (no SET_IMAGES, APPEND_IMAGES,
 * FILTER_IMAGES, CLEAR_DATASET, or COMMIT_GEOMETRY allowed).
 */
export function isTransitionFrozen(stage: TransitionStage): boolean {
  return stage === 'fading_out';
}

/** Whether any transition is in progress (not idle). */
export function isTransitionActive(stage: TransitionStage): boolean {
  return stage !== 'idle';
}
