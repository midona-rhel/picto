/**
 * Canvas2D grid renderer — dual-canvas architecture with QoS thumbnail pipeline.
 *
 * Restored from legacy v0.5.0-alpha with stability fixes:
 * - No scroll metrics cache (always fresh DOM reads)
 * - Frame-coherent viewport snapshots
 * - Debounced resize (16ms)
 * - Atomic layout + visibility recomputation
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode } from '../layout/types';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { drawCanvasBaseLayer, type DrawContext } from './drawBase';

import { buildCanvasVisibilityPlan } from './visibilityPlan';
import { ThumbnailPipeline } from './thumbnailPipeline';
import { adaptGridItem } from './renderItemAdapter';
import { hitTestTile } from './hitTesting';
import {
  type CanvasScrollState,
  createIdleCanvasScrollState,
  classifyCanvasScrollPhase,
  resolveCanvasScrollDirection,
  CANVAS_SCROLL_IDLE_DELAY_MS,
} from './scrollState';
import { useCanvasRedrawScheduler } from './useCanvasRedrawScheduler';
import { snapshotViewport, ensureCanvasSize } from './canvasViewportUtils';
import styles from './CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const LOAD_MORE_THRESHOLD_PX = 400;

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
  const baseCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const pipelineRef = useRef<ThumbnailPipeline | null>(null);
  /** Per-tile reveal start time. Tiles enter → get a timestamp. Tiles leave → removed. */
  const revealMapRef = useRef(new Map<number, number>());
  const scrollStateRef = useRef<CanvasScrollState>(createIdleCanvasScrollState());
  const lastScrollTopRef = useRef(0);
  const lastScrollTimeRef = useRef(0);
  const idleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const firstPaintRef = useRef(false);
  const onLoadMoreRef = useRef(onLoadMore);
  const onScrollTopChangeRef = useRef(onScrollTopChange);
  onLoadMoreRef.current = onLoadMore;
  onScrollTopChangeRef.current = onScrollTopChange;

  // Debounced container dimensions for layout — prevents jitter during resize
  const [containerWidth, setContainerWidth] = useState(0);
  const [containerHeight, setContainerHeight] = useState(0);
  const [scrollbarWidth, setScrollbarWidth] = useState(0);
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;

  // ── Render items (adapted from entities) ──
  const renderItems = useMemo(
    () => items.map(adaptGridItem),
    [items],
  );

  const aspectRatios = useMemo(
    () => items.map((item) =>
      safeAspectRatio(item.pixel_width && item.pixel_height ? item.pixel_width / item.pixel_height : 1.5),
    ),
    [items],
  );

  // ── Layout (recomputed on width/targetSize/viewMode change, NOT on scroll) ──
  const layout = useMemo(
    () => computeLayout(aspectRatios, containerWidth, targetSize, GAP, viewMode, textHeight, 0, scrollbarWidth),
    [aspectRatios, containerWidth, targetSize, GAP, viewMode, textHeight, scrollbarWidth],
  );

  // ── Thumbnail pipeline lifecycle ──
  useEffect(() => {
    const pipeline = new ThumbnailPipeline(() => {
      markDirty('base');
    });
    pipelineRef.current = pipeline;
    return () => {
      pipelineRef.current = null;
    };
  }, []);

  // Reset pipeline generation on item list change
  useEffect(() => {
    firstPaintRef.current = false;
    revealMapRef.current.clear();
  }, [items[0]?.entity_hash]);

  // ── Draw functions ──
  const drawBase = useCallback(() => {
    const container = containerRef.current;
    const canvas = baseCanvasRef.current;
    const pipeline = pipelineRef.current;
    if (!container || !canvas || !pipeline) return;

    const vp = snapshotViewport(container);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    ensureCanvasSize(canvas, vp.containerWidth, vp.viewportHeight, vp.dpr);

    const scrollTop = interactive ? vp.scrollTop : frozenScrollTop;
    const scrollState = scrollStateRef.current;

    const plan = buildCanvasVisibilityPlan({
      positions: layout.positions,
      scrollTop,
      viewportHeight: vp.viewportHeight,
      scrollPhase: scrollState.phase,
      scrollDirection: scrollState.direction,
      queueDepth: pipeline.getStats().queueDepth,
    });

    ctx.save();
    ctx.scale(vp.dpr, vp.dpr);
    ctx.clearRect(0, 0, vp.containerWidth, vp.viewportHeight);

    // Per-tile reveal map: tracks when each tile entered the activation zone.
    // - Added when tile enters the visible+prefetch range
    // - Removed when tile is fully outside the activation zone
    // - Cleared on items/scope change
    const ACTIVATION_MARGIN = 400;
    const now = performance.now();
    const revealMap = revealMapRef.current;
    const zoneTop = scrollTop - ACTIVATION_MARGIN;
    const zoneBottom = scrollTop + vp.viewportHeight + ACTIVATION_MARGIN;

    // Add tiles in the draw range
    for (let n = 0; n < plan.visibleIterEnd; n++) {
      const i = plan.visibleIndices ? plan.visibleIndices[n] : plan.startIdx + n;
      if (i >= plan.endIdx) break;
      if (!revealMap.has(i)) {
        revealMap.set(i, now);
      }
    }
    for (const idx of plan.prefetchIndices) {
      if (!revealMap.has(idx)) {
        revealMap.set(idx, now);
      }
    }

    // Remove tiles fully outside activation zone
    for (const idx of revealMap.keys()) {
      const pos = layout.positions[idx];
      if (!pos) { revealMap.delete(idx); continue; }
      if (pos.y + pos.h < zoneTop || pos.y > zoneBottom) {
        revealMap.delete(idx);
      }
    }

    const drawCtx: DrawContext = {
      scrollTop,
      viewportHeight: vp.viewportHeight,
      textHeight,
      borderRadius: 4,
    };

    const hasActiveReveal = drawCanvasBaseLayer({
      ctx,
      positions: layout.positions,
      items: renderItems,
      atlasGet: (hash) => pipeline.get(hash),
      atlasEnsure: (hash, args) => {
        pipeline.ensure(hash, args);
      },
      now,
      revealMap,
      plan,
      draw: drawCtx,
      theme: {
        placeholderBg: 'rgba(255, 255, 255, 0.04)',
        borderRadius: 4,
        textPrimary: 'rgba(255, 255, 255, 0.92)',
        textTertiary: 'rgba(255, 255, 255, 0.36)',
      },
      viewMode,
      showTileName: showName,
      showResolution: false,
      showExtension,
      showExtensionLabel: showExtension,
    });

    ctx.restore();

    // Prefetch
    for (const idx of plan.prefetchIndices) {
      const item = renderItems[idx];
      if (item) {
        pipeline.ensure(item.thumbnailHash);
      }
    }

    // Cancel outside window
    pipeline.cancelOutsideWindow(plan.cancelTop, plan.cancelBottom);

    // Continue animation loop for active reveals
    if (hasActiveReveal) {
      markDirty('base');
    }

    // First paint notification
    if (!firstPaintRef.current && plan.visibleIterEnd > 0) {
      firstPaintRef.current = true;
      onFirstPaint?.();
    }
  }, [layout, renderItems, viewMode, showName, showExtension, textHeight, interactive, frozenScrollTop]);

  const drawOverlay = useCallback(() => {
    const container = containerRef.current;
    const canvas = overlayCanvasRef.current;
    if (!container || !canvas) return;

    const vp = snapshotViewport(container);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    ensureCanvasSize(canvas, vp.containerWidth, vp.viewportHeight, vp.dpr);
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    // TODO: selection borders, hover rings, marquee
  }, [layout]);

  // ── RAF scheduler (legacy pattern) ──
  const frozenRef = useRef(!interactive);
  frozenRef.current = !interactive;
  const drawBaseRef = useRef(drawBase);
  drawBaseRef.current = drawBase;
  const drawOverlayRef = useRef(drawOverlay);
  drawOverlayRef.current = drawOverlay;
  const { markDirty } = useCanvasRedrawScheduler({
    frozenRef,
    drawBaseRef,
    drawOverlayRef,
  });

  // ── Resize observer (debounced 16ms) ──
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const measure = () => {
      const w = container.clientWidth;
      const h = container.clientHeight;
      const sbw = container.offsetWidth - w;
      setContainerWidth(w);
      setContainerHeight(h);
      setScrollbarWidth(sbw);
    };

    measure();
    const observer = new ResizeObserver(() => {
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
      resizeTimerRef.current = setTimeout(() => {
        resizeTimerRef.current = null;
        measure();
      }, 16);
    });
    observer.observe(container);
    return () => {
      observer.disconnect();
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
    };
  }, []);

  // ── Redraw on layout/prop changes ──
  useEffect(() => { markDirty('both'); }, [layout, markDirty]);
  useEffect(() => { markDirty('base'); }, [showName, showExtension, viewMode, suppressTileReveal, markDirty]);

  // ── Scroll handler ──
  const handleScroll = useCallback(() => {
    const container = containerRef.current;
    if (!container || !interactive) return;

    const now = performance.now();
    const scrollTop = container.scrollTop;
    const delta = scrollTop - lastScrollTopRef.current;
    const elapsed = now - lastScrollTimeRef.current;
    const velocity = elapsed > 0 ? (Math.abs(delta) / elapsed) * 1000 : 0;

    lastScrollTopRef.current = scrollTop;
    lastScrollTimeRef.current = now;

    const nextScrollState: CanvasScrollState = {
      phase: classifyCanvasScrollPhase(velocity),
      direction: resolveCanvasScrollDirection(delta),
      velocityPxPerSec: velocity,
    };
    scrollStateRef.current = nextScrollState;
    pipelineRef.current?.setScrollState(nextScrollState);

    onScrollTopChangeRef.current?.(scrollTop);
    markDirty('both');

    // Transition to idle after inactivity
    if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
    idleTimerRef.current = setTimeout(() => {
      scrollStateRef.current = createIdleCanvasScrollState();
      markDirty('base'); // Idle enables more prefetch
    }, CANVAS_SCROLL_IDLE_DELAY_MS);

    // Load more trigger
    const distanceFromBottom = container.scrollHeight - scrollTop - container.clientHeight;
    if (distanceFromBottom < LOAD_MORE_THRESHOLD_PX) {
      onLoadMoreRef.current?.();
    }
  }, [interactive, markDirty]);

  // ── Frozen scroll ──
  useEffect(() => {
    if (!interactive && containerRef.current) {
      containerRef.current.scrollTop = frozenScrollTop;
      markDirty('both');
    }
  }, [frozenScrollTop, interactive, markDirty]);

  // ── Click handler ──
  const handleClick = useCallback((e: React.MouseEvent) => {
    if (!onTileClick) return;
    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top + container.scrollTop;

    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);
    if (idx != null && items[idx]) {
      onTileClick(idx, items[idx]);
    }
  }, [items, layout.positions, onTileClick, textHeight]);

  // ── Render ──
  return (
    <div className={styles.root}>
      <div
        ref={containerRef}
        className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}
        onScroll={handleScroll}
        onClick={handleClick}
      >
        <div
          className={styles.canvasWrap}
          style={{ height: `${layout.totalHeight}px` }}
        >
          <div
            className={styles.viewportLayer}
            style={{ height: `${containerHeight}px` }}
          >
            <canvas
              ref={baseCanvasRef}
              style={{ display: 'block', width: '100%' }}
            />
            <canvas
              ref={overlayCanvasRef}
              style={{
                position: 'absolute',
                inset: 0,
                display: 'block',
                width: '100%',
                pointerEvents: 'none',
              }}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
