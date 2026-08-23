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

export function createNativeDragImageUrl(
  hashes: string[],
  count: number,
  getBitmap: (hash: string) => ImageBitmap | null,
): string {
  const stackCount = dragGhostStackCount(hashes.length);
  const width = DRAG_GHOST_THUMB_SIZE + (stackCount - 1) * DRAG_GHOST_STACK_OFFSET + 14;
  const height = DRAG_GHOST_THUMB_SIZE + (stackCount - 1) * DRAG_GHOST_STACK_OFFSET + 8;
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  if (!context) return '';

  for (let i = 0; i < stackCount; i++) {
    const bitmap = getBitmap(hashes[i]);
    if (!bitmap) continue;
    const x = i * DRAG_GHOST_STACK_OFFSET;
    const y = i * DRAG_GHOST_STACK_OFFSET + 6;
    context.save();
    context.beginPath();
    context.roundRect(x, y, DRAG_GHOST_THUMB_SIZE, DRAG_GHOST_THUMB_SIZE, DRAG_GHOST_RADIUS);
    context.clip();
    context.drawImage(bitmap, x, y, DRAG_GHOST_THUMB_SIZE, DRAG_GHOST_THUMB_SIZE);
    context.restore();
    context.strokeStyle = DRAG_GHOST_BORDER;
    context.lineWidth = 1;
    context.beginPath();
    context.roundRect(x + 0.5, y + 0.5, DRAG_GHOST_THUMB_SIZE - 1, DRAG_GHOST_THUMB_SIZE - 1, DRAG_GHOST_RADIUS);
    context.stroke();
  }

  if (count > 1) {
    const label = formatDragGhostCount(count);
    context.font = 'bold 10px -apple-system, BlinkMacSystemFont, sans-serif';
    const badgeWidth = Math.max(DRAG_GHOST_BADGE_MIN_WIDTH, context.measureText(label).width + 10);
    context.fillStyle = '#3297FF';
    context.beginPath();
    context.roundRect(width - badgeWidth, 0, badgeWidth, DRAG_GHOST_BADGE_HEIGHT, DRAG_GHOST_BADGE_HEIGHT / 2);
    context.fill();
    context.fillStyle = 'white';
    context.textAlign = 'center';
    context.textBaseline = 'middle';
    context.fillText(label, width - badgeWidth / 2, DRAG_GHOST_BADGE_HEIGHT / 2);
  }
  return canvas.toDataURL('image/png');
}
