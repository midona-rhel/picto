import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties, ImgHTMLAttributes, MouseEvent as ReactMouseEvent } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode } from '../layout/types';
import { computeLayout, lowerBound, safeAspectRatio } from '../layout/layoutMath';
import { mediaThumbnailUrl } from './mediaUrl';
import { formatDuration, isHiddenBadgeType, mimeToExt } from './primitives';
import styles from './CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const PADDING_X = GAP;
const SCROLL_IDLE_DELAY_MS = 80;
const IDLE_OVERSCAN_VIEWPORTS = 1;
const ACTIVE_OVERSCAN_VIEWPORTS = 0.35;
const IDLE_LOAD_BATCH = 12;

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

function measureContainerSize(container: HTMLDivElement): { width: number; height: number; scrollbarWidth: number } {
  const rect = container.getBoundingClientRect();
  const width = container.clientWidth || Math.round(rect.width);
  const height = container.clientHeight || Math.round(rect.height);
  const scrollbarWidth = Math.max(0, container.offsetWidth - width);
  return { width, height, scrollbarWidth };
}

function objectFitFor(viewMode: GridViewMode, mime: string): CSSProperties['objectFit'] {
  if (mime.startsWith('video/')) return 'contain';
  return viewMode === 'grid' ? 'contain' : 'cover';
}

function TileImage({ src, alt, suppressReveal, className, ...rest }: {
  src: string;
  alt: string;
  suppressReveal: boolean;
  className: string;
} & Omit<ImgHTMLAttributes<HTMLImageElement>, 'src' | 'alt' | 'className'>) {
  const [loaded, setLoaded] = useState(false);
  const frameRef = useRef<number | null>(null);

  const reveal = useCallback(() => {
    if (frameRef.current != null) {
      cancelAnimationFrame(frameRef.current);
    }
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      setLoaded(true);
    });
  }, []);

  useEffect(() => {
    setLoaded(false);
    return () => {
      if (frameRef.current != null) {
        cancelAnimationFrame(frameRef.current);
      }
    };
  }, [src]);

  return (
    <img
      {...rest}
      src={src}
      alt={alt}
      className={`${className} ${loaded || suppressReveal ? styles.tileImageLoaded : ''}`}
      onLoad={() => {
        if (suppressReveal) {
          setLoaded(true);
          return;
        }
        reveal();
      }}
      ref={(node) => {
        if (!node || !node.complete) return;
        if (suppressReveal) {
          setLoaded(true);
          return;
        }
        reveal();
      }}
      draggable={false}
    />
  );
}

function HtmlTile({
  item,
  index,
  x,
  y,
  w,
  h,
  viewMode,
  showName,
  showExtension,
  suppressTileReveal,
  imageEnabled,
  onTileClick,
}: {
  item: CanonicalEntityGridItem;
  index: number;
  x: number;
  y: number;
  w: number;
  h: number;
  viewMode: GridViewMode;
  showName: boolean;
  showExtension: boolean;
  suppressTileReveal: boolean;
  imageEnabled: boolean;
  onTileClick?: (index: number, item: CanonicalEntityGridItem) => void;
}) {
  const imageHeight = showName ? Math.max(0, h - TEXT_NAME_ROW_H) : h;
  const fit = objectFitFor(viewMode, item.mime_type);
  const ext = mimeToExt(item.mime_type);
  const showExtensionBadge = showExtension && !isHiddenBadgeType(ext.toLowerCase());
  const tileStyle: CSSProperties = {
    left: `${x}px`,
    top: `${y}px`,
    width: `${w}px`,
    height: `${h}px`,
  };
  const imageWrapStyle: CSSProperties = {
    height: `${imageHeight}px`,
    backgroundColor: item.dominant_color_hex ?? 'rgba(255, 255, 255, 0.04)',
  };

  const handleClick = (event: ReactMouseEvent<HTMLButtonElement>) => {
    event.preventDefault();
    onTileClick?.(index, item);
  };

  return (
    <button type="button" className={styles.tile} style={tileStyle} onClick={handleClick}>
      <div className={styles.tileImageWrap} style={imageWrapStyle}>
        {item.has_thumbnail && imageEnabled ? (
          <TileImage
            src={mediaThumbnailUrl(item.thumbnail_hash)}
            alt={item.name ?? ''}
            suppressReveal={suppressTileReveal}
            className={styles.tileImage}
            style={{ objectFit: fit }}
            loading="lazy"
            decoding="async"
          />
        ) : null}

        {item.duration_ms != null && (
          <div className={`${styles.badge} ${styles.badgeTopRight}`}>{formatDuration(item.duration_ms)}</div>
        )}
        {item.entity_kind === 'collection' && item.member_count != null && (
          <div className={`${styles.badge} ${item.duration_ms != null ? styles.badgeTopRightOffset : styles.badgeTopRight}`}>
            {item.member_count}
          </div>
        )}
        {showExtensionBadge && (
          <div className={`${styles.badge} ${styles.badgeBottomRight}`}>{ext}</div>
        )}
        {item.rating != null && item.rating > 0 && (
          <div className={styles.rating}>{'★'.repeat(item.rating)}</div>
        )}
      </div>

      {showName && item.name && <div className={styles.tileName}>{item.name}</div>}
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
  const firstPaintNotifiedRef = useRef(false);
  const scrollFrameRef = useRef<number | null>(null);
  const scrollIdleTimerRef = useRef<number | null>(null);
  const loadFlushTimerRef = useRef<number | null>(null);
  const latestScrollTopRef = useRef(0);
  const scrollActiveRef = useRef(false);
  const pendingVisibleLoadsRef = useRef<Set<string>>(new Set());
  const onLoadMoreRef = useRef(onLoadMore);
  const onFirstPaintRef = useRef(onFirstPaint);
  const onScrollTopChangeRef = useRef(onScrollTopChange);

  const [containerSize, setContainerSize] = useState({ width: 0, height: 0, scrollbarWidth: 0 });
  const [scrollTop, setScrollTop] = useState(0);
  const [scrollActive, setScrollActive] = useState(false);
  const [enabledImageHashes, setEnabledImageHashes] = useState<Set<string>>(() => new Set());

  onLoadMoreRef.current = onLoadMore;
  onFirstPaintRef.current = onFirstPaint;
  onScrollTopChangeRef.current = onScrollTopChange;

  const flushPendingVisibleLoads = useCallback(() => {
    loadFlushTimerRef.current = null;
    if (scrollActiveRef.current) return;
    const pending = pendingVisibleLoadsRef.current;
    if (pending.size === 0) return;

    const nextHashes = Array.from(pending).slice(0, IDLE_LOAD_BATCH);
    if (nextHashes.length === 0) return;

    setEnabledImageHashes((prev) => {
      const next = new Set(prev);
      for (const hash of nextHashes) {
        next.add(hash);
        pending.delete(hash);
      }
      return next;
    });

    if (pending.size > 0) {
      loadFlushTimerRef.current = window.setTimeout(flushPendingVisibleLoads, 32);
    }
  }, []);

  const scheduleLoadFlush = useCallback(() => {
    if (scrollActiveRef.current) return;
    if (loadFlushTimerRef.current != null) return;
    loadFlushTimerRef.current = window.setTimeout(flushPendingVisibleLoads, 0);
  }, [flushPendingVisibleLoads]);

  const aspectRatios = useMemo(
    () => items.map((item) => safeAspectRatio(
      item.pixel_width && item.pixel_height ? item.pixel_width / item.pixel_height : 1.5,
    )),
    [items],
  );

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;
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

  const effectiveScrollTop = interactive ? scrollTop : frozenScrollTop;
  const visibleRange = useMemo(() => {
    if (layout.positions.length === 0 || containerSize.height <= 0) {
      return { startIdx: 0, endIdx: 0 };
    }
    const overscanPx = containerSize.height * (scrollActive ? ACTIVE_OVERSCAN_VIEWPORTS : IDLE_OVERSCAN_VIEWPORTS);
    const top = Math.max(0, effectiveScrollTop - overscanPx);
    const bottom = effectiveScrollTop + containerSize.height + overscanPx;
    return {
      startIdx: Math.max(0, lowerBound(layout.positions, top) - 1),
      endIdx: Math.min(layout.positions.length, lowerBound(layout.positions, bottom) + 1),
    };
  }, [containerSize.height, effectiveScrollTop, layout.positions, scrollActive]);

  const visibleItems = useMemo(() => {
    const next: Array<{ item: CanonicalEntityGridItem; index: number; pos: { x: number; y: number; w: number; h: number } }> = [];
    for (let i = visibleRange.startIdx; i < visibleRange.endIdx; i += 1) {
      const pos = layout.positions[i];
      const item = items[i];
      if (!pos || !item) continue;
      next.push({ item, index: i, pos });
    }
    return next;
  }, [items, layout.positions, visibleRange.endIdx, visibleRange.startIdx]);

  const showLoadingSpinner = useMemo(
    () => visibleItems.some(({ item }) => item.has_thumbnail && !enabledImageHashes.has(item.thumbnail_hash)),
    [enabledImageHashes, visibleItems],
  );

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const syncSize = () => {
      const next = measureContainerSize(container);
      setContainerSize((prev) => (
        prev.width === next.width
        && prev.height === next.height
        && prev.scrollbarWidth === next.scrollbarWidth
      ) ? prev : next);
    };

    syncSize();
    const observer = new ResizeObserver(syncSize);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    if (!interactive) {
      container.scrollTop = frozenScrollTop;
      setScrollTop(frozenScrollTop);
    }
  }, [frozenScrollTop, interactive]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !interactive) return;

    const flushScroll = () => {
      scrollFrameRef.current = null;
      setScrollTop(latestScrollTopRef.current);
      onScrollTopChangeRef.current?.(latestScrollTopRef.current);
    };

    const handleScroll = () => {
      latestScrollTopRef.current = container.scrollTop;
      scrollActiveRef.current = true;
      setScrollActive(true);

      if (scrollFrameRef.current == null) {
        scrollFrameRef.current = requestAnimationFrame(flushScroll);
      }

      if (scrollIdleTimerRef.current != null) {
        window.clearTimeout(scrollIdleTimerRef.current);
      }
      scrollIdleTimerRef.current = window.setTimeout(() => {
        scrollIdleTimerRef.current = null;
        scrollActiveRef.current = false;
        setScrollActive(false);
        scheduleLoadFlush();
      }, SCROLL_IDLE_DELAY_MS);

      const { scrollHeight, clientHeight } = container;
      if (scrollHeight - container.scrollTop - clientHeight < 400) {
        onLoadMoreRef.current?.();
      }
    };

    container.addEventListener('scroll', handleScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', handleScroll);
      if (scrollFrameRef.current != null) {
        cancelAnimationFrame(scrollFrameRef.current);
        scrollFrameRef.current = null;
      }
      if (scrollIdleTimerRef.current != null) {
        window.clearTimeout(scrollIdleTimerRef.current);
        scrollIdleTimerRef.current = null;
      }
    };
  }, [interactive, scheduleLoadFlush]);

  useEffect(() => {
    const pending = pendingVisibleLoadsRef.current;
    let hasNewPending = false;

    for (const { item } of visibleItems) {
      if (!item.has_thumbnail) continue;
      if (enabledImageHashes.has(item.thumbnail_hash)) continue;
      if (pending.has(item.thumbnail_hash)) continue;
      pending.add(item.thumbnail_hash);
      hasNewPending = true;
    }

    if (hasNewPending && !scrollActive) {
      scheduleLoadFlush();
    }
  }, [enabledImageHashes, scheduleLoadFlush, scrollActive, visibleItems]);

  useEffect(() => {
    firstPaintNotifiedRef.current = false;
  }, [items, viewMode, targetSize, showName, showExtension]);

  useEffect(() => {
    if (firstPaintNotifiedRef.current) return;
    if (items.length === 0 || visibleItems.length === 0) return;
    firstPaintNotifiedRef.current = true;
    onFirstPaintRef.current?.();
  }, [items.length, visibleItems.length]);

  useEffect(() => () => {
    if (loadFlushTimerRef.current != null) {
      window.clearTimeout(loadFlushTimerRef.current);
    }
  }, []);

  return (
    <div ref={containerRef} className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}>
      <div className={styles.spacer} style={{ height: `${layout.totalHeight}px` }}>
        {visibleItems.map(({ item, index, pos }) => (
          <HtmlTile
            key={`${item.entity_hash}:${Math.round(pos.y)}`}
            item={item}
            index={index}
            x={pos.x}
            y={pos.y}
            w={pos.w}
            h={pos.h}
            viewMode={viewMode}
            showName={showName}
            showExtension={showExtension}
            suppressTileReveal={suppressTileReveal}
            imageEnabled={enabledImageHashes.has(item.thumbnail_hash)}
            onTileClick={onTileClick}
          />
        ))}
      </div>
      {showLoadingSpinner ? (
        <div className={styles.spinnerOverlay} aria-hidden="true">
          <div className={styles.spinner} />
        </div>
      ) : null}
    </div>
  );
}
