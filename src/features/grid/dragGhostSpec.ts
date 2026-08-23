export const DRAG_GHOST_THUMB_SIZE = 44;
export const DRAG_GHOST_STACK_OFFSET = 3;
export const DRAG_GHOST_RADIUS = 4;
export const DRAG_GHOST_BORDER = 'rgba(255,255,255,0.2)';
export const DRAG_GHOST_SHADOW = '0 2px 8px rgba(0,0,0,0.3)';
export const DRAG_GHOST_BADGE_HEIGHT = 20;
export const DRAG_GHOST_BADGE_MIN_WIDTH = 20;

export function dragGhostStackCount(thumbnailCount: number): number {
  return Math.min(thumbnailCount, 3);
}

export function formatDragGhostCount(count: number): string {
  return String(count);
}
