import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useSetAtom } from 'jotai';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { gridPerfAtom } from '../../../state/gridPerf';
import { gridFrameTraceAtom } from '../../../state/gridTrace';
import type { LayoutItem } from '../layout/types';
import type { GridViewMode } from '../layout/types';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { mediaThumbnailUrl, mimeToMediaExtension } from './mediaUrl';
import { adaptGridItem } from './renderItemAdapter';
import styles from './CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const PADDING_X = GAP;
const LOAD_MORE_THRESHOLD_PX = 400;
const ROW_OVERSCAN = 8;
const PLACEHOLDER_BG = 'rgba(255, 255, 255, 0.04)';

interface CanvasGridProps {
  items: CanonicalEntityGridItem[];
  viewMode: GridViewMode;
  targetSize: number;
  showName: boolean;
  showExtension: boolean;
  onTileClick?: (index: number, item: CanonicalEntityGridItem) => void;
  onLoadMore?: () => void;
  onFirstPaint?: () => void;
  onScrollTopChange?: (scrollTop: number) => void;
  interactive?: boolean;
  frozenScrollTop?: number;
  suppressTileReveal?: boolean;
}

interface VirtualBand {
  start: number;
  size: number;
  indices: number[];
}

function measureContainerSize(container: HTMLDivElement): { width: number; height: number } {
  const rect = container.getBoundingClientRect();
  const width = container.clientWidth || Math.round(rect.width);
  const height = container.clientHeight || Math.round(rect.height);
  return { width, height };
}

function buildVirtualBands(positions: LayoutItem[], totalHeight: number): VirtualBand[] {
  if (positions.length === 0) return [];

  const starts = positions
    .map((pos, index) => ({ start: pos.y, index }))
    .sort((a, b) => a.start - b.start || a.index - b.index);

  const groups: Array<{ start: number; indices: number[] }> = [];
  for (const entry of starts) {
    const last = groups[groups.length - 1];
    if (last && Math.abs(last.start - entry.start) < 0.5) {
      last.indices.push(entry.index);
    } else {
      groups.push({ start: entry.start, indices: [entry.index] });
    }
  }

  return groups.map((group, index) => {
    const nextStart = index + 1 < groups.length ? groups[index + 1].start : totalHeight;
    return {
      start: group.start,
      size: Math.max(1, nextStart - group.start),
      indices: group.indices,
    };
  });
}

function formatDuration(durationMs: number): string {
  const totalSeconds = Math.max(0, Math.floor(durationMs / 1000));
  const seconds = totalSeconds % 60;
  const minutes = Math.floor(totalSeconds / 60) % 60;
  const hours = Math.floor(totalSeconds / 3600);

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  }
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

interface GridTileProps {
  index: number;
  item: CanonicalEntityGridItem;
  position: LayoutItem;
  showName: boolean;
  showExtension: boolean;
  viewMode: GridViewMode;
  suppressTileReveal: boolean;
  onTileClick?: (index: number, item: CanonicalEntityGridItem) => void;
}

function GridTile({
  index,
  item,
  position,
  showName,
  showExtension,
  viewMode,
  suppressTileReveal,
  onTileClick,
}: GridTileProps) {
  const renderItem = adaptGridItem(item);
  const [loaded, setLoaded] = useState(false);
  const imageHeight = Math.max(0, position.h - (showName ? TEXT_NAME_ROW_H : 0));
  const fitMode = viewMode === 'grid' || renderItem.mime.startsWith('video/') ? 'contain' : 'cover';
  const imageSrc = item.has_thumbnail ? mediaThumbnailUrl(renderItem.thumbnailHash) : null;
  const placeholderColor = renderItem.dominantColor ?? PLACEHOLDER_BG;
  const extension = showExtension ? mimeToMediaExtension(renderItem.mime).toUpperCase() : null;

  useEffect(() => {
    setLoaded(false);
  }, [imageSrc]);

  return (
    <button
      type="button"
      className={styles.tile}
      style={{
        transform: `translate3d(${position.x}px, ${position.y}px, 0)`,
        width: `${position.w}px`,
        height: `${position.h}px`,
      }}
      onClick={() => onTileClick?.(index, item)}
    >
      <div className={styles.mediaFrame} style={{ height: `${imageHeight}px`, backgroundColor: placeholderColor }}>
        {imageSrc && (
          <img
            alt={renderItem.name ?? ''}
            className={`${styles.media} ${fitMode === 'contain' ? styles.mediaContain : styles.mediaCover} ${loaded || suppressTileReveal ? styles.mediaLoaded : ''}`}
            src={imageSrc}
            draggable={false}
            decoding="async"
            loading="lazy"
            onLoad={() => setLoaded(true)}
          />
        )}

        <div className={styles.badgesTopRight}>
          {renderItem.durationMs != null && <span className={styles.badge}>{formatDuration(renderItem.durationMs)}</span>}
          {renderItem.kind === 'collection' && renderItem.memberCount != null && (
            <span className={styles.badge}>{renderItem.memberCount}</span>
          )}
        </div>

        {extension && <span className={`${styles.badge} ${styles.badgeBottomRight}`}>{extension}</span>}
      </div>

      {showName && (
        <div className={styles.metaRow}>
          <span className={styles.nameText}>{renderItem.name ?? ''}</span>
        </div>
      )}
    </button>
  );
}

export function CanvasGrid({
  items,
  viewMode,
  targetSize,
  showName,
  showExtension,
  onTileClick,
  onLoadMore,
  onFirstPaint,
  onScrollTopChange,
  interactive = true,
  frozenScrollTop = 0,
  suppressTileReveal = false,
}: CanvasGridProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const setGridPerf = useSetAtom(gridPerfAtom);
  const setGridFrameTrace = useSetAtom(gridFrameTraceAtom);
  const firstPaintNotifiedRef = useRef(false);
  const onLoadMoreRef = useRef(onLoadMore);
  const onScrollTopChangeRef = useRef(onScrollTopChange);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0, scrollbarWidth: 0 });

  onLoadMoreRef.current = onLoadMore;
  onScrollTopChangeRef.current = onScrollTopChange;

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;
  const aspectRatios = useMemo(
    () => items.map((item) => safeAspectRatio(item.pixel_width && item.pixel_height ? item.pixel_width / item.pixel_height : 1.5)),
    [items],
  );

  const layout = useMemo(() => computeLayout(
    aspectRatios,
    containerSize.width,
    targetSize,
    GAP,
    viewMode,
    textHeight,
    PADDING_X,
    containerSize.scrollbarWidth,
  ), [aspectRatios, containerSize.scrollbarWidth, containerSize.width, targetSize, textHeight, viewMode]);

  const bands = useMemo(
    () => buildVirtualBands(layout.positions, layout.totalHeight),
    [layout.positions, layout.totalHeight],
  );

  const rowVirtualizer = useVirtualizer({
    count: bands.length,
    getScrollElement: () => containerRef.current,
    estimateSize: (index) => bands[index]?.size ?? Math.max(targetSize, 1),
    overscan: ROW_OVERSCAN,
  });

  useEffect(() => {
    rowVirtualizer.measure();
  }, [bands, rowVirtualizer]);

  const virtualRows = rowVirtualizer.getVirtualItems();
  const visibleIndices = useMemo(() => {
    const indexSet = new Set<number>();
    for (const row of virtualRows) {
      const band = bands[row.index];
      if (!band) continue;
      for (const index of band.indices) indexSet.add(index);
    }
    return Array.from(indexSet).sort((a, b) => a - b);
  }, [bands, virtualRows]);

  const maybeLoadMore = useCallback(() => {
    const container = containerRef.current;
    const loadMore = onLoadMoreRef.current;
    if (!container || !loadMore) return;
    if (container.scrollHeight - container.scrollTop - container.clientHeight < LOAD_MORE_THRESHOLD_PX) {
      loadMore();
    }
  }, []);

  useEffect(() => {
    setGridPerf(null);
    setGridFrameTrace(null);
    return () => {
      setGridPerf(null);
      setGridFrameTrace(null);
    };
  }, [setGridFrameTrace, setGridPerf]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const updateSize = () => {
      const { width, height } = measureContainerSize(container);
      setContainerSize({
        width,
        height,
        scrollbarWidth: container.offsetWidth - width,
      });
    };

    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!interactive && containerRef.current) {
      containerRef.current.scrollTop = frozenScrollTop;
      onScrollTopChangeRef.current?.(frozenScrollTop);
    }
  }, [frozenScrollTop, interactive]);

  useEffect(() => {
    firstPaintNotifiedRef.current = false;
  }, [items[0]?.entity_hash]);

  useEffect(() => {
    if (firstPaintNotifiedRef.current) return;
    if (items.length === 0 || visibleIndices.length === 0) return;
    firstPaintNotifiedRef.current = true;
    onFirstPaint?.();
  }, [items.length, onFirstPaint, visibleIndices.length]);

  useEffect(() => {
    maybeLoadMore();
  }, [layout.totalHeight, maybeLoadMore, visibleIndices.length]);

  const handleScroll = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    onScrollTopChangeRef.current?.(container.scrollTop);
    maybeLoadMore();
  }, [maybeLoadMore]);

  return (
    <div className={styles.root}>
      <div
        ref={containerRef}
        className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}
        onScroll={handleScroll}
      >
        <div className={styles.canvasWrap} style={{ height: `${layout.totalHeight}px` }}>
          {visibleIndices.map((index) => {
            const position = layout.positions[index];
            const item = items[index];
            if (!position || !item) return null;
            return (
              <GridTile
                key={item.entity_hash}
                index={index}
                item={item}
                position={position}
                showName={showName}
                showExtension={showExtension}
                viewMode={viewMode}
                suppressTileReveal={suppressTileReveal}
                onTileClick={onTileClick}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}
