import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { WheelEvent as ReactWheelEvent } from 'react';
import { useSetAtom } from 'jotai';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { gridPerfAtom } from '../../../state/gridPerf';
import { gridFrameTraceAtom } from '../../../state/gridTrace';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import type { LayoutItem, GridViewMode } from '../layout/types';
import { mediaThumbnailUrl } from './mediaUrl';
import {
  formatDuration,
  getContainRect,
  isHiddenBadgeType,
  mimeToExt,
} from './primitives';
import { adaptGridItem } from './renderItemAdapter';
import { CANVAS_SCROLL_IDLE_DELAY_MS } from './scrollState';
import styles from './CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const PADDING_X = GAP;
const LOAD_MORE_THRESHOLD_PX = 400;
const PLACEHOLDER_BG = 'rgba(255, 255, 255, 0.04)';
const MIN_OVERSCAN_PX = 600;
const SCROLL_VISIBLE_HYDRATION_BATCH = 2;
const VISIBLE_HYDRATION_BATCH = 6;
const PREFETCH_HYDRATION_BATCH = 12;
const FADE_STAGGER_STEP_MS = 24;
const FADE_STAGGER_MAX_MS = 160;
const MODULO_STRESS_TEST_ITEM_COUNT = 999_999;
const MAX_NATIVE_SCROLL_HEIGHT = 8_000_000;
const COMPRESSED_WHEEL_DELTA_FACTOR = 0.45;

type IdleHandle = ReturnType<typeof globalThis.setTimeout> | number | null;

export interface CanvasGridProps {
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

interface SortedLayoutEntry {
  index: number;
  y: number;
  bottom: number;
}

interface VisibleRange {
  renderedIndices: number[];
  visibleIndices: number[];
  renderedHashes: Set<string>;
  visibleHashes: Set<string>;
  prefetchHashes: Set<string>;
}

interface GridTileProps {
  index: number;
  item: CanonicalEntityGridItem;
  position: LayoutItem;
  showName: boolean;
  showExtension: boolean;
  viewMode: GridViewMode;
  imageEnabled: boolean;
  imageLoaded: boolean;
  fadeDelayMs: number;
  suppressTileReveal: boolean;
  onTileClick?: (index: number, item: CanonicalEntityGridItem) => void;
  onImageLoad: (hash: string) => void;
}

function measureContainerSize(container: HTMLDivElement): { width: number; height: number } {
  const rect = container.getBoundingClientRect();
  const width = container.clientWidth || Math.round(rect.width);
  const height = container.clientHeight || Math.round(rect.height);
  return { width, height };
}

function lowerBoundEntries(
  entries: SortedLayoutEntry[],
  target: number,
  selector: (entry: SortedLayoutEntry) => number,
): number {
  let lo = 0;
  let hi = entries.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (selector(entries[mid]) < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function buildVisibleRange(args: {
  entries: SortedLayoutEntry[];
  getItemAtIndex: (index: number) => CanonicalEntityGridItem | null;
  scrollTop: number;
  viewportHeight: number;
}): VisibleRange {
  const { entries, getItemAtIndex, scrollTop, viewportHeight } = args;
  if (entries.length === 0 || viewportHeight <= 0) {
    return {
      renderedIndices: [],
      visibleIndices: [],
      renderedHashes: new Set<string>(),
      visibleHashes: new Set<string>(),
      prefetchHashes: new Set<string>(),
    };
  }

  const overscanPx = Math.max(viewportHeight, MIN_OVERSCAN_PX);
  const renderTop = scrollTop - overscanPx;
  const renderBottom = scrollTop + viewportHeight + overscanPx;
  const visibleTop = scrollTop;
  const visibleBottom = scrollTop + viewportHeight;
  const renderStart = Math.max(0, lowerBoundEntries(entries, renderTop, (entry) => entry.bottom) - 1);
  const renderEnd = Math.min(entries.length, lowerBoundEntries(entries, renderBottom, (entry) => entry.y) + 1);

  const renderedIndices: number[] = [];
  const visibleIndices: number[] = [];
  const renderedHashes = new Set<string>();
  const visibleHashes = new Set<string>();
  const prefetchHashes = new Set<string>();

  for (let i = renderStart; i < renderEnd; i += 1) {
    const entry = entries[i];
    if (!entry) continue;
    if (entry.bottom < renderTop) continue;
    if (entry.y > renderBottom) break;

    renderedIndices.push(entry.index);
    const item = getItemAtIndex(entry.index);
    if (!item) continue;
    renderedHashes.add(item.thumbnail_hash);

    const isVisible = entry.bottom >= visibleTop && entry.y <= visibleBottom;
    if (isVisible) {
      visibleIndices.push(entry.index);
      visibleHashes.add(item.thumbnail_hash);
    } else {
      prefetchHashes.add(item.thumbnail_hash);
    }
  }

  return {
    renderedIndices,
    visibleIndices,
    renderedHashes,
    visibleHashes,
    prefetchHashes,
  };
}

function requestIdleWork(callback: () => void): IdleHandle {
  if ('requestIdleCallback' in window) {
    return (
      window as Window & {
        requestIdleCallback: (cb: () => void, options?: { timeout: number }) => number;
      }
    ).requestIdleCallback(callback, { timeout: 50 });
  }
  return globalThis.setTimeout(callback, 0);
}

function normalizeWheelDelta(event: ReactWheelEvent<HTMLDivElement>): number {
  if (event.deltaMode === 1) return event.deltaY * 16;
  if (event.deltaMode === 2) return event.deltaY * (window.innerHeight || 800);
  return event.deltaY;
}

function cancelIdleWork(handle: IdleHandle): void {
  if (handle == null) return;
  if ('cancelIdleCallback' in window) {
    (
      window as Window & {
        cancelIdleCallback: (id: number) => void;
      }
    ).cancelIdleCallback(handle as number);
    return;
  }
  globalThis.clearTimeout(handle);
}

function GridTile({
  index,
  item,
  position,
  showName,
  showExtension,
  viewMode,
  imageEnabled,
  imageLoaded,
  fadeDelayMs,
  suppressTileReveal,
  onTileClick,
  onImageLoad,
}: GridTileProps) {
  const renderItem = adaptGridItem(item);
  const imageHeight = Math.max(0, position.h - (showName ? TEXT_NAME_ROW_H : 0));
  const useContain = viewMode === 'grid' || renderItem.mime.startsWith('video/');
  const imageSrc = imageEnabled && item.has_thumbnail ? mediaThumbnailUrl(renderItem.thumbnailHash) : null;
  const placeholderColor = renderItem.dominantColor ?? PLACEHOLDER_BG;
  const extension = showExtension ? mimeToExt(renderItem.mime) : '';
  const showExtensionBadge = !!extension && !isHiddenBadgeType(extension);
  const containRect = useContain
    ? getContainRect(renderItem.aspectRatio ?? 1, 0, 0, position.w, imageHeight)
    : null;
  const [revealed, setRevealed] = useState(suppressTileReveal);

  useEffect(() => {
    if (!imageSrc) {
      setRevealed(false);
      return;
    }
    if (suppressTileReveal) {
      setRevealed(true);
      return;
    }
    setRevealed(false);
  }, [imageSrc, suppressTileReveal]);

  useEffect(() => {
    if (!imageSrc || !imageLoaded) return undefined;
    if (suppressTileReveal) {
      setRevealed(true);
      return undefined;
    }
    const frame = window.requestAnimationFrame(() => {
      setRevealed(true);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [imageLoaded, imageSrc, suppressTileReveal]);

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
      <div
        className={styles.mediaFrame}
        style={{
          height: `${imageHeight}px`,
          backgroundColor: useContain ? PLACEHOLDER_BG : placeholderColor,
        }}
      >
        {useContain && containRect ? (
          <div
            className={styles.containPlaceholder}
            style={{
              left: `${containRect.x}px`,
              top: `${containRect.y}px`,
              width: `${containRect.w}px`,
              height: `${containRect.h}px`,
              backgroundColor: placeholderColor,
            }}
          />
        ) : (
          <div className={styles.coverPlaceholder} style={{ backgroundColor: placeholderColor }} />
        )}

        {imageSrc && (
          <img
            alt={renderItem.name ?? ''}
            className={`${styles.media} ${useContain ? styles.mediaContain : styles.mediaCover} ${
              revealed ? styles.mediaLoaded : ''
            }`}
            style={{ transitionDelay: suppressTileReveal ? '0ms' : `${fadeDelayMs}ms` }}
            src={imageSrc}
            draggable={false}
            decoding="async"
            loading="lazy"
            onLoad={() => onImageLoad(renderItem.thumbnailHash)}
          />
        )}

        <span className={`${styles.badge} ${styles.badgeTopLeft}`}>{index + 1}</span>

        <div className={styles.badgesTopRight}>
          {renderItem.durationMs != null && (
            <span className={styles.badge}>{formatDuration(renderItem.durationMs)}</span>
          )}
          {renderItem.kind === 'collection' && renderItem.memberCount != null && (
            <span className={styles.badge}>{String(renderItem.memberCount)}</span>
          )}
        </div>

        {showExtensionBadge && (
          <span className={`${styles.badge} ${styles.badgeBottomRight}`}>{extension}</span>
        )}
      </div>

      {showName && (
        <div className={styles.metaRow}>
          <span className={styles.nameText}>{renderItem.name ?? ''}</span>
        </div>
      )}
    </button>
  );
}

export function DomGridFallback({
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
  const onLoadMoreRef = useRef(onLoadMore);
  const onScrollTopChangeRef = useRef(onScrollTopChange);
  const onFirstPaintRef = useRef(onFirstPaint);
  const scrollActiveRef = useRef(false);
  const scrollIdleTimerRef = useRef<number | null>(null);
  const idleHydrationHandleRef = useRef<IdleHandle>(null);
  const logicalScrollTopRef = useRef(frozenScrollTop);
  const enabledImageHashesRef = useRef<Set<string>>(new Set());
  const loadedImageHashesRef = useRef<Set<string>>(new Set());
  const pendingVisibleHydrationRef = useRef<Set<string>>(new Set());
  const pendingPrefetchHydrationRef = useRef<Set<string>>(new Set());
  const retentionHashesRef = useRef<Set<string>>(new Set());
  const firstPaintNotifiedRef = useRef(false);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0, scrollbarWidth: 0 });
  const [scrollTop, setScrollTop] = useState(frozenScrollTop);
  const [enabledImageHashes, setEnabledImageHashes] = useState<Set<string>>(() => new Set());
  const [loadedImageHashes, setLoadedImageHashes] = useState<Set<string>>(() => new Set());
  const setGridPerf = useSetAtom(gridPerfAtom);
  const setGridFrameTrace = useSetAtom(gridFrameTraceAtom);

  onLoadMoreRef.current = onLoadMore;
  onScrollTopChangeRef.current = onScrollTopChange;
  onFirstPaintRef.current = onFirstPaint;
  enabledImageHashesRef.current = enabledImageHashes;
  loadedImageHashesRef.current = loadedImageHashes;
  logicalScrollTopRef.current = scrollTop;

  const effectiveItemCount = items.length > 0 ? MODULO_STRESS_TEST_ITEM_COUNT : 0;
  const getItemAtIndex = useCallback((index: number) => {
    if (items.length === 0) return null;
    return items[index % items.length] ?? null;
  }, [items]);

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;
  const aspectRatios = useMemo(
    () => Array.from({ length: effectiveItemCount }, (_, index) => {
      const item = getItemAtIndex(index);
      return safeAspectRatio(
        item?.pixel_width && item.pixel_height ? item.pixel_width / item.pixel_height : 1.5,
      );
    }),
    [effectiveItemCount, getItemAtIndex],
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

  const sortedEntries = useMemo<SortedLayoutEntry[]>(
    () => layout.positions
      .map((position, index) => ({
        index,
        y: position.y,
        bottom: position.y + position.h,
      }))
      .sort((a, b) => a.y - b.y || a.index - b.index),
    [layout.positions],
  );

  const maxLogicalScrollTop = Math.max(0, layout.totalHeight - containerSize.height);
  const physicalTotalHeight = Math.max(
    containerSize.height,
    Math.min(layout.totalHeight, MAX_NATIVE_SCROLL_HEIGHT),
  );
  const maxNativeScrollTop = Math.max(0, physicalTotalHeight - containerSize.height);
  const usesCompressedScroll = layout.totalHeight > MAX_NATIVE_SCROLL_HEIGHT;

  const logicalToNativeScrollTop = useCallback((logicalTop: number) => {
    if (maxLogicalScrollTop <= 0 || maxNativeScrollTop <= 0) return 0;
    const clampedLogicalTop = Math.min(Math.max(logicalTop, 0), maxLogicalScrollTop);
    return (clampedLogicalTop / maxLogicalScrollTop) * maxNativeScrollTop;
  }, [maxLogicalScrollTop, maxNativeScrollTop]);

  const nativeToLogicalScrollTop = useCallback((nativeTop: number) => {
    if (maxLogicalScrollTop <= 0 || maxNativeScrollTop <= 0) return 0;
    const clampedNativeTop = Math.min(Math.max(nativeTop, 0), maxNativeScrollTop);
    return (clampedNativeTop / maxNativeScrollTop) * maxLogicalScrollTop;
  }, [maxLogicalScrollTop, maxNativeScrollTop]);

  const activeScrollTop = interactive ? scrollTop : Math.min(frozenScrollTop, maxLogicalScrollTop);
  const visibleRange = useMemo(
    () => buildVisibleRange({
      entries: sortedEntries,
      getItemAtIndex,
      scrollTop: activeScrollTop,
      viewportHeight: containerSize.height,
    }),
    [activeScrollTop, containerSize.height, getItemAtIndex, sortedEntries],
  );

  const visibleFadeDelayByHash = useMemo(() => {
    const delays = new Map<string, number>();
    for (let i = 0; i < visibleRange.visibleIndices.length; i += 1) {
      const item = getItemAtIndex(visibleRange.visibleIndices[i]);
      if (!item || delays.has(item.thumbnail_hash)) continue;
      delays.set(item.thumbnail_hash, Math.min(i * FADE_STAGGER_STEP_MS, FADE_STAGGER_MAX_MS));
    }
    return delays;
  }, [getItemAtIndex, visibleRange.visibleIndices]);

  const scheduleHydrationFlush = useCallback(() => {
    if (idleHydrationHandleRef.current != null) return;

    idleHydrationHandleRef.current = requestIdleWork(() => {
      idleHydrationHandleRef.current = null;
      const scrollActive = scrollActiveRef.current;

      let changed = false;
      const nextEnabled = new Set(enabledImageHashesRef.current);

      let visibleBudget = scrollActive ? SCROLL_VISIBLE_HYDRATION_BATCH : VISIBLE_HYDRATION_BATCH;
      for (const hash of Array.from(pendingVisibleHydrationRef.current)) {
        if (visibleBudget <= 0) break;
        pendingVisibleHydrationRef.current.delete(hash);
        if (nextEnabled.has(hash)) continue;
        nextEnabled.add(hash);
        visibleBudget -= 1;
        changed = true;
      }

      let prefetchBudget = scrollActive ? 0 : PREFETCH_HYDRATION_BATCH;
      for (const hash of Array.from(pendingPrefetchHydrationRef.current)) {
        if (prefetchBudget <= 0) break;
        pendingPrefetchHydrationRef.current.delete(hash);
        if (nextEnabled.has(hash)) continue;
        nextEnabled.add(hash);
        prefetchBudget -= 1;
        changed = true;
      }

      for (const hash of Array.from(nextEnabled)) {
        if (!retentionHashesRef.current.has(hash)) {
          nextEnabled.delete(hash);
          changed = true;
        }
      }

      if (changed) {
        enabledImageHashesRef.current = nextEnabled;
        setEnabledImageHashes(nextEnabled);
      }

      const hasVisiblePending = pendingVisibleHydrationRef.current.size > 0;
      const hasPrefetchPending = pendingPrefetchHydrationRef.current.size > 0;
      if (hasVisiblePending || (!scrollActive && hasPrefetchPending)) {
        scheduleHydrationFlush();
      }
    });
  }, []);

  const cancelHydrationFlush = useCallback(() => {
    if (idleHydrationHandleRef.current == null) return;
    cancelIdleWork(idleHydrationHandleRef.current);
    idleHydrationHandleRef.current = null;
  }, []);

  const maybeLoadMore = useCallback((nextScrollTop?: number) => {
    const loadMore = onLoadMoreRef.current;
    if (!loadMore) return;
    const currentScrollTop = nextScrollTop ?? activeScrollTop;
    if (layout.totalHeight - currentScrollTop - containerSize.height < LOAD_MORE_THRESHOLD_PX) {
      loadMore();
    }
  }, [activeScrollTop, containerSize.height, layout.totalHeight]);

  const handleImageLoad = useCallback((hash: string) => {
    if (loadedImageHashesRef.current.has(hash)) return;
    const next = new Set(loadedImageHashesRef.current);
    next.add(hash);
    loadedImageHashesRef.current = next;
    setLoadedImageHashes(next);
  }, []);

  const scheduleScrollIdle = useCallback(() => {
    if (scrollIdleTimerRef.current != null) window.clearTimeout(scrollIdleTimerRef.current);
    scrollIdleTimerRef.current = window.setTimeout(() => {
      scrollIdleTimerRef.current = null;
      scrollActiveRef.current = false;
      scheduleHydrationFlush();
    }, CANVAS_SCROLL_IDLE_DELAY_MS);
  }, [scheduleHydrationFlush]);

  const applyLogicalScrollTop = useCallback((nextScrollTop: number) => {
    const clampedTop = Math.min(Math.max(nextScrollTop, 0), maxLogicalScrollTop);
    logicalScrollTopRef.current = clampedTop;
    setScrollTop(clampedTop);
    onScrollTopChangeRef.current?.(clampedTop);
    maybeLoadMore(clampedTop);
    scheduleHydrationFlush();
    scheduleScrollIdle();
  }, [maxLogicalScrollTop, maybeLoadMore, scheduleHydrationFlush, scheduleScrollIdle]);

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
    setGridPerf(null);
    setGridFrameTrace(null);
    return () => {
      setGridPerf(null);
      setGridFrameTrace(null);
    };
  }, [setGridFrameTrace, setGridPerf]);

  useEffect(() => {
    if (interactive) return;
    const clampedFrozenTop = Math.min(frozenScrollTop, maxLogicalScrollTop);
    logicalScrollTopRef.current = clampedFrozenTop;
    setScrollTop(clampedFrozenTop);
    if (containerRef.current) {
      containerRef.current.scrollTop = logicalToNativeScrollTop(clampedFrozenTop);
    }
    onScrollTopChangeRef.current?.(clampedFrozenTop);
  }, [frozenScrollTop, interactive, logicalToNativeScrollTop, maxLogicalScrollTop]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const targetLogicalTop = Math.min(activeScrollTop, maxLogicalScrollTop);
    if (interactive && targetLogicalTop !== scrollTop) {
      logicalScrollTopRef.current = targetLogicalTop;
      setScrollTop(targetLogicalTop);
      return;
    }
    const targetNativeTop = logicalToNativeScrollTop(targetLogicalTop);
    if (Math.abs(container.scrollTop - targetNativeTop) > 1) {
      container.scrollTop = targetNativeTop;
    }
  }, [activeScrollTop, interactive, logicalToNativeScrollTop, maxLogicalScrollTop, scrollTop]);

  useEffect(() => {
    firstPaintNotifiedRef.current = false;
  }, [items[0]?.entity_hash]);

  useEffect(() => {
    retentionHashesRef.current = new Set(visibleRange.renderedHashes);

    for (const hash of visibleRange.visibleHashes) {
      if (!enabledImageHashesRef.current.has(hash)) {
        pendingVisibleHydrationRef.current.add(hash);
      }
      pendingPrefetchHydrationRef.current.delete(hash);
    }

    for (const hash of visibleRange.prefetchHashes) {
      if (!enabledImageHashesRef.current.has(hash) && !pendingVisibleHydrationRef.current.has(hash)) {
        pendingPrefetchHydrationRef.current.add(hash);
      }
    }

    for (const hash of Array.from(pendingVisibleHydrationRef.current)) {
      if (!retentionHashesRef.current.has(hash)) pendingVisibleHydrationRef.current.delete(hash);
    }
    for (const hash of Array.from(pendingPrefetchHydrationRef.current)) {
      if (!retentionHashesRef.current.has(hash)) pendingPrefetchHydrationRef.current.delete(hash);
    }

    scheduleHydrationFlush();
  }, [scheduleHydrationFlush, visibleRange.prefetchHashes, visibleRange.renderedHashes, visibleRange.visibleHashes]);

  useEffect(() => {
    if (firstPaintNotifiedRef.current) return;
    if (effectiveItemCount === 0 || visibleRange.visibleIndices.length === 0) return;
    firstPaintNotifiedRef.current = true;
    onFirstPaintRef.current?.();
  }, [effectiveItemCount, visibleRange.visibleIndices.length]);

  useEffect(() => {
    maybeLoadMore(activeScrollTop);
  }, [activeScrollTop, layout.totalHeight, maybeLoadMore]);

  useEffect(() => () => {
    if (scrollIdleTimerRef.current != null) window.clearTimeout(scrollIdleTimerRef.current);
    cancelHydrationFlush();
  }, [cancelHydrationFlush]);

  const handleScroll = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    scrollActiveRef.current = true;
    applyLogicalScrollTop(nativeToLogicalScrollTop(container.scrollTop));
  }, [applyLogicalScrollTop, nativeToLogicalScrollTop]);

  const handleWheel = useCallback((event: ReactWheelEvent<HTMLDivElement>) => {
    if (!usesCompressedScroll || !interactive) return;
    const rawDeltaY = normalizeWheelDelta(event);
    const maxStep = Math.max(120, containerSize.height * 0.35);
    const deltaY = Math.max(
      -maxStep,
      Math.min(maxStep, rawDeltaY * COMPRESSED_WHEEL_DELTA_FACTOR),
    );
    if (deltaY === 0) return;
    event.preventDefault();
    scrollActiveRef.current = true;
    applyLogicalScrollTop(logicalScrollTopRef.current + deltaY);
  }, [applyLogicalScrollTop, containerSize.height, interactive, usesCompressedScroll]);

  return (
    <div className={styles.root}>
      <div
        ref={containerRef}
        className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}
        onScroll={interactive ? handleScroll : undefined}
        onWheel={interactive ? handleWheel : undefined}
      >
        <div className={styles.canvasWrap} style={{ height: `${physicalTotalHeight}px` }}>
          <div className={styles.viewportLayer} style={{ height: `${containerSize.height}px` }}>
            {visibleRange.renderedIndices.map((index) => {
              const position = layout.positions[index];
              const item = getItemAtIndex(index);
              if (!position || !item) return null;
              return (
                <GridTile
                  key={`${item.entity_hash}:${index}`}
                  index={index}
                  item={item}
                  position={{
                    ...position,
                    y: position.y - activeScrollTop,
                  }}
                  showName={showName}
                  showExtension={showExtension}
                  viewMode={viewMode}
                  imageEnabled={enabledImageHashes.has(item.thumbnail_hash)}
                  imageLoaded={loadedImageHashes.has(item.thumbnail_hash)}
                  fadeDelayMs={visibleFadeDelayByHash.get(item.thumbnail_hash) ?? 0}
                  suppressTileReveal={suppressTileReveal}
                  onTileClick={onTileClick}
                  onImageLoad={handleImageLoad}
                />
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
