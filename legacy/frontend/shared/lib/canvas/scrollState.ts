export type CanvasScrollPhase = 'idle' | 'slow' | 'fast';
export type CanvasScrollDirection = 'forward' | 'backward' | 'unknown';

export interface CanvasScrollState {
  phase: CanvasScrollPhase;
  direction: CanvasScrollDirection;
  velocityPxPerSec: number;
}

export const CANVAS_SCROLL_IDLE_DELAY_MS = 80;
export const CANVAS_SCROLL_FAST_THRESHOLD_PX_PER_SEC = 1800;

export function classifyCanvasScrollPhase(velocityPxPerSec: number): CanvasScrollPhase {
  if (velocityPxPerSec >= CANVAS_SCROLL_FAST_THRESHOLD_PX_PER_SEC) return 'fast';
  if (velocityPxPerSec > 0) return 'slow';
  return 'idle';
}

export function resolveCanvasScrollDirection(deltaPx: number): CanvasScrollDirection {
  if (deltaPx > 0) return 'forward';
  if (deltaPx < 0) return 'backward';
  return 'unknown';
}

export function createIdleCanvasScrollState(): CanvasScrollState {
  return {
    phase: 'idle',
    direction: 'unknown',
    velocityPxPerSec: 0,
  };
}
