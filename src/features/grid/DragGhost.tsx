import { createPortal } from 'react-dom';
import {
  DRAG_GHOST_BADGE_HEIGHT,
  DRAG_GHOST_BADGE_MIN_WIDTH,
  DRAG_GHOST_BORDER,
  DRAG_GHOST_RADIUS,
  DRAG_GHOST_SHADOW,
  DRAG_GHOST_STACK_OFFSET,
  DRAG_GHOST_THUMB_SIZE,
  dragGhostStackCount,
  formatDragGhostCount,
} from './dragGhostSpec';

interface DragGhostProps {
  x: number;
  y: number;
  thumbnailHashes: string[];
  count: number;
}

export function DragGhost({ x, y, thumbnailHashes, count }: DragGhostProps) {
  const stackCount = dragGhostStackCount(thumbnailHashes.length);
  const thumbs = thumbnailHashes.slice(0, 3);

  return createPortal(
    <div style={{
      position: 'fixed',
      left: x - 24,
      top: y - 24,
      zIndex: 10000,
      pointerEvents: 'none',
      opacity: 0.85,
    }}>
      <div style={{ position: 'relative', width: 48 + (stackCount - 1) * DRAG_GHOST_STACK_OFFSET, height: 48 + (stackCount - 1) * DRAG_GHOST_STACK_OFFSET }}>
        {thumbs.map((hash, i) => (
          <img
            key={hash}
            src={`media://localhost/thumb/${hash}.jpg`}
            draggable={false}
            style={{
              position: 'absolute',
              top: i * DRAG_GHOST_STACK_OFFSET,
              left: i * DRAG_GHOST_STACK_OFFSET,
              width: DRAG_GHOST_THUMB_SIZE,
              height: DRAG_GHOST_THUMB_SIZE,
              objectFit: 'cover',
              borderRadius: DRAG_GHOST_RADIUS,
              border: `1px solid ${DRAG_GHOST_BORDER}`,
              boxShadow: DRAG_GHOST_SHADOW,
            }}
          />
        ))}
        {count > 1 && (
          <div style={{
            position: 'absolute',
            top: -6,
            right: -6,
            background: 'var(--color-primary)',
            color: '#fff',
            borderRadius: 10,
            minWidth: DRAG_GHOST_BADGE_MIN_WIDTH,
            height: DRAG_GHOST_BADGE_HEIGHT,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            fontSize: 10,
            fontWeight: 600,
            padding: '0 5px',
          }}>
            {formatDragGhostCount(count)}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}
