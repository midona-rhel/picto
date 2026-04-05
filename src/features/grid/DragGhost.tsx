/**
 * DragGhost — floating stacked thumbnail preview during grid tile drag.
 * Matches legacy v0.5.0-alpha exactly: up to 3 stacked 44x44 thumbnails + count badge.
 * Rendered via portal to document.body to escape overflow clipping.
 */

import { createPortal } from 'react-dom';

interface DragGhostProps {
  x: number;
  y: number;
  thumbnailHashes: string[];
  count: number;
}

export function DragGhost({ x, y, thumbnailHashes, count }: DragGhostProps) {
  const stackCount = Math.min(thumbnailHashes.length, 3);
  const thumbs = thumbnailHashes.slice(0, 3);

  return createPortal(
    <div style={{
      position: 'fixed',
      left: x + 14,
      top: y + 14,
      zIndex: 10000,
      pointerEvents: 'none',
      opacity: 0.85,
    }}>
      <div style={{ position: 'relative', width: 48 + (stackCount - 1) * 3, height: 48 + (stackCount - 1) * 3 }}>
        {thumbs.map((hash, i) => (
          <img
            key={hash}
            src={`media://localhost/thumb/${hash}.jpg`}
            draggable={false}
            style={{
              position: 'absolute',
              top: i * 3,
              left: i * 3,
              width: 44,
              height: 44,
              objectFit: 'cover',
              borderRadius: 4,
              border: '1px solid rgba(255, 255, 255, 0.2)',
              boxShadow: '0 2px 8px rgba(0, 0, 0, 0.3)',
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
            minWidth: 20,
            height: 20,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            fontSize: 10,
            fontWeight: 600,
            padding: '0 5px',
          }}>
            {count}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}
