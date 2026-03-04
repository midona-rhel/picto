import { useImageDrag } from '../../lib/imageDrag';

/**
 * Floating drag ghost — renders 1-3 stacked thumbnails
 * at the cursor position with a count badge.
 */
export function DragGhost() {
  const drag = useImageDrag();
  if (!drag) return null;

  const count = drag.hashes.length;
  const thumbs = drag.thumbnailUrls;
  const stackCount = Math.min(thumbs.length, 3);

  return (
    <div
      style={{
        position: 'fixed',
        left: drag.x + 14,
        top: drag.y + 14,
        zIndex: 10000,
        pointerEvents: 'none',
        opacity: 0.85,
      }}
    >
      <div style={{ position: 'relative', width: 48 + (stackCount - 1) * 3, height: 48 + (stackCount - 1) * 3 }}>
        {thumbs.slice(0, 3).map((url, i) => (
          <img
            key={i}
            src={url}
            draggable={false}
            style={{
              position: 'absolute',
              top: i * 3,
              left: i * 3,
              width: 44,
              height: 44,
              objectFit: 'cover',
              borderRadius: 4,
              border: '1px solid var(--color-white-20)',
              boxShadow: '0 2px 8px var(--color-black-30)',
            }}
          />
        ))}
      </div>
      {count > 1 && (
        <div
          style={{
            position: 'absolute',
            top: -6,
            right: -6,
            background: 'var(--color-primary)',
            color: 'var(--color-white-99)',
            borderRadius: 10,
            minWidth: 20,
            height: 20,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            fontSize: 'var(--font-size-2xs)',
            fontWeight: 'var(--font-weight-bold)' as any,
            padding: '0 5px',
          }}
        >
          {count}
        </div>
      )}
    </div>
  );
}
