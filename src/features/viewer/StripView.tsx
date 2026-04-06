/**
 * StripView — full-quality collection member browsing.
 *
 * Two layout modes:
 *   - horizontal: single column, each image spans full width, scroll vertically
 *   - vertical: each row is viewport-height, images pack left-to-right
 *
 * Uses DOM rendering with native lazy loading for performance.
 */

import { useEffect, useRef, useMemo, useCallback } from 'react';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import { mediaFileUrl } from '../../shared/lib/mediaUrl';
import styles from './StripView.module.css';

function formatDuration(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

export type StripFitMode = 'horizontal' | 'vertical';

interface StripViewProps {
  items: CanonicalEntityGridItem[];
  fitMode: StripFitMode;
  onItemClick: (index: number, item: CanonicalEntityGridItem) => void;
  onLoadMore?: () => void;
  selectedHash?: string | null;
  initialIndex?: number;
}

// ── Vertical mode row packing ──

interface PackedRow {
  items: { item: CanonicalEntityGridItem; index: number }[];
}

function packVerticalRows(
  items: CanonicalEntityGridItem[],
  containerWidth: number,
  rowHeight: number,
  gap: number,
): PackedRow[] {
  const rows: PackedRow[] = [];
  let currentRow: PackedRow = { items: [] };
  let usedWidth = 0;

  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    const aspect = (item.pixel_width && item.pixel_height)
      ? item.pixel_width / item.pixel_height
      : 1.5;
    const imgWidth = rowHeight * aspect;
    const neededWidth = usedWidth > 0 ? imgWidth + gap : imgWidth;

    if (usedWidth > 0 && usedWidth + neededWidth > containerWidth) {
      rows.push(currentRow);
      currentRow = { items: [] };
      usedWidth = 0;
    }

    currentRow.items.push({ item, index: i });
    usedWidth += usedWidth > 0 ? imgWidth + gap : imgWidth;
  }

  if (currentRow.items.length > 0) rows.push(currentRow);
  return rows;
}

// ── Component ──

export function StripView({
  items, fitMode, onItemClick, onLoadMore, selectedHash, initialIndex,
}: StripViewProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const scrolledRef = useRef(false);

  // Scroll to initial index on mount
  useEffect(() => {
    if (initialIndex == null || initialIndex < 0 || scrolledRef.current) return;
    const el = rootRef.current?.querySelector(`[data-strip-index="${initialIndex}"]`);
    if (el) {
      el.scrollIntoView({ block: 'center' });
      scrolledRef.current = true;
    }
  }, [initialIndex, items.length]);

  // Load more via IntersectionObserver
  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!sentinel || !onLoadMore) return;
    const observer = new IntersectionObserver(
      ([entry]) => { if (entry.isIntersecting) onLoadMore(); },
      { rootMargin: '400px' },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [onLoadMore]);

  // Container dimensions for vertical mode
  const containerWidth = rootRef.current?.clientWidth ?? 800;
  const viewportHeight = rootRef.current?.clientHeight ?? 600;

  // Vertical row packing
  const packedRows = useMemo(
    () => fitMode === 'vertical'
      ? packVerticalRows(items, containerWidth - 24, viewportHeight - 24, 12)
      : [],
    [items, containerWidth, viewportHeight, fitMode],
  );

  const handleClick = useCallback((index: number) => {
    const item = items[index];
    if (item) onItemClick(index, item);
  }, [items, onItemClick]);

  const renderImage = useCallback((item: CanonicalEntityGridItem, index: number) => {
    const isVideo = item.mime_type.startsWith('video/');
    const isSelected = selectedHash === item.entity_hash;

    return (
      <div
        key={item.entity_hash}
        data-strip-index={index}
        className={`${styles.imageWrap} ${isSelected ? styles.imageWrapSelected : ''}`}
        style={{ backgroundColor: item.dominant_color_hex ?? undefined }}
        onClick={() => handleClick(index)}
      >
        <img
          src={mediaFileUrl(item.entity_hash, item.mime_type)}
          loading="lazy"
          alt=""
          draggable={false}
        />
        {isVideo && (
          <>
            <div className={styles.playBadge}>&#9654;</div>
            {item.duration_ms != null && item.duration_ms > 0 && (
              <div className={styles.durationBadge}>{formatDuration(item.duration_ms)}</div>
            )}
          </>
        )}
      </div>
    );
  }, [selectedHash, handleClick]);

  return (
    <div ref={rootRef} className={styles.root}>
      {fitMode === 'horizontal' ? (
        <div className={styles.horizontalStack}>
          {items.map((item, i) => renderImage(item, i))}
        </div>
      ) : (
        <div className={styles.verticalRows}>
          {packedRows.map((row, ri) => (
            <div key={ri} className={styles.verticalRow} style={{ height: viewportHeight - 24 }}>
              {row.items.map(({ item, index }) => renderImage(item, index))}
            </div>
          ))}
        </div>
      )}
      <div ref={sentinelRef} className={styles.sentinel} />
    </div>
  );
}
