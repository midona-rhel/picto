/**
 * Canvas2D grid renderer — dual-canvas (base + overlay) with thumbnail pipeline.
 *
 * Activation zone: viewport ± 100px. Tiles inside are drawn, loaded, and
 * fade-animated. Tiles outside get loads cancelled and stop rendering.
 * One linear scan per frame drives everything.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode } from '../layout/types';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { HoverPreviewPortal } from './HoverPreviewPortal';
import { drawCanvasBaseLayer, type DrawContext } from './drawBase';

import { ThumbnailPipeline, REVEAL_DURATION_MS } from './thumbnailPipeline';
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

const GAP = 16;
const TEXT_NAME_ROW_H = 20;
const LOAD_MORE_THRESHOLD_PX = 400;
const ZOOM_BTN_SIZE = 24;
const HOVER_PREVIEW_DELAY_MS = 200;
const HOVER_HIDE_DELAY_MS = 90;

export interface CanvasGridProps {
  items: CanonicalEntityGridItem[];
  viewMode: GridViewMode;
  targetSize: number;
  showName: boolean;
  showExtension: boolean;
  showResolution?: boolean;
  fitThumbnails?: boolean;
  /** Total item count for the current scope (from backend). Used to estimate scroll height before all items are loaded. */
  totalCount?: number | null;
  onTileClick?: (index: number, item: CanonicalEntityGridItem, event?: React.MouseEvent) => void;
  onTileDoubleClick?: (index: number, item: CanonicalEntityGridItem) => void;
  onEmptyClick?: () => void;
  onTileContextMenu?: (index: number, item: CanonicalEntityGridItem, position: { x: number; y: number }) => void;
  onEmptyContextMenu?: (position: { x: number; y: number }) => void;
  onLoadMore?: () => void;
  onFirstPaint?: () => void;
  onScrollTopChange?: (scrollTop: number) => void;
  interactive?: boolean;
  frozenScrollTop?: number;
  suppressTileReveal?: boolean;
  /** Restore scroll position on first paint (e.g., after back/forward navigation). */
  initialScrollTop?: number | null;
  selectedEntityHashes?: Set<string>;
  onSelectionChange?: (hashes: Set<string>) => void;
}

export function CanvasGrid({
  items,
  viewMode,
  targetSize,
  showName,
  showExtension,
  showResolution = false,
  fitThumbnails = false,
  totalCount = null,
  onTileClick,
  onTileDoubleClick,
  onEmptyClick,
  onTileContextMenu,
  onEmptyContextMenu,
  onSelectionChange,
  onLoadMore,
  onFirstPaint,
  onScrollTopChange,
  interactive = true,
  frozenScrollTop = 0,
  suppressTileReveal = false,
  initialScrollTop = null,
  selectedEntityHashes = new Set<string>(),
}: CanvasGridProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const baseCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const pipelineRef = useRef<ThumbnailPipeline | null>(null);
  const scrollStateRef = useRef<CanvasScrollState>(createIdleCanvasScrollState());
  const lastScrollTopRef = useRef(0);
  const lastScrollTimeRef = useRef(0);
  const idleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const prevLayoutRef = useRef<typeof layout | null>(null);
  const prevItemsRef = useRef(items);
  const hoveredTileRef = useRef<number | null>(null);
  const isScrollingRef = useRef(false);
  const marqueeRef = useRef<{ startX: number; startY: number; active: boolean; shiftKey: boolean; lastClientX: number; lastClientY: number }>({
    startX: 0, startY: 0, active: false, shiftKey: false, lastClientX: 0, lastClientY: 0,
  });
  const marqueeRectRef = useRef<{ left: number; top: number; width: number; height: number } | null>(null);
  const marqueeBaseSelectionRef = useRef<Set<string>>(new Set());
  const dragJustEndedRef = useRef(false);
  const autoScrollRef = useRef<number | null>(null);
  const autoScrollSpeedRef = useRef(0);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [hoverPreview, setHoverPreview] = useState<{ hash: string; mime: string } | null>(null);
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

  const effectiveViewMode = viewMode;
  const textHeight = (showName ? TEXT_NAME_ROW_H : 0) + (showResolution ? TEXT_NAME_ROW_H : 0);

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
    () => computeLayout(aspectRatios, containerWidth, targetSize, GAP, effectiveViewMode, textHeight, 0, scrollbarWidth),
    [aspectRatios, containerWidth, targetSize, GAP, effectiveViewMode, textHeight, scrollbarWidth],
  );

  // ── Scroll anchor on resize/zoom ──
  // Each step picks fresh: prefer selected item closest to center, else any tile.
  useEffect(() => {
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = layout;
    const prevItems = prevItemsRef.current;
    prevItemsRef.current = items;

    if (prevItems !== items) return;
    if (!prev || prev === layout || prev.positions === layout.positions) return;
    if (prev.positions.length === 0 || layout.positions.length === 0) return;
    const container = containerRef.current;
    if (!container) return;

    const scrollTop = container.scrollTop;
    const vh = container.clientHeight;
    if (vh === 0) return;

    const viewportCenter = scrollTop + vh / 2;
    let anchorIdx = -1;
    let bestDist = Infinity;

    // Prefer selected item closest to viewport center
    if (selectedEntityHashes.size > 0) {
      for (let i = 0; i < prev.positions.length; i++) {
        if (!items[i] || !selectedEntityHashes.has(items[i].entity_hash)) continue;
        const dist = Math.abs(prev.positions[i].y + prev.positions[i].h / 2 - viewportCenter);
        if (dist < bestDist) { bestDist = dist; anchorIdx = i; }
      }
    }

    // Fall back to any tile closest to center
    if (anchorIdx < 0) {
      for (let i = 0; i < prev.positions.length; i++) {
        const dist = Math.abs(prev.positions[i].y + prev.positions[i].h / 2 - viewportCenter);
        if (dist < bestDist) { bestDist = dist; anchorIdx = i; }
      }
    }

    if (anchorIdx < 0 || anchorIdx >= layout.positions.length) return;

    const oldCenter = prev.positions[anchorIdx].y + prev.positions[anchorIdx].h / 2;
    const offsetInViewport = oldCenter - scrollTop;
    const newCenter = layout.positions[anchorIdx].y + layout.positions[anchorIdx].h / 2;
    container.scrollTop = Math.max(0, newCenter - offsetInViewport);
  }, [layout, items, selectedEntityHashes]);

  // ── Estimated total scroll height ──
  // When totalCount > loaded items, estimate from average height per item.
  // First page loads 500 items — enough for a good estimate.
  // As more 100-item batches load, the estimate refines imperceptibly.
  const estimatedTotalHeight = useMemo(() => {
    const loadedCount = items.length;
    const total = totalCount ?? loadedCount;
    if (total <= loadedCount || loadedCount === 0) return layout.totalHeight;
    const avgPerItem = layout.totalHeight / loadedCount;
    return Math.round(avgPerItem * total);
  }, [items.length, totalCount, layout.totalHeight]);

  // ── Thumbnail pipeline lifecycle ──
  useEffect(() => {
    const pipeline = new ThumbnailPipeline(() => {
      markDirty('base');
    });
    pipelineRef.current = pipeline;
    return () => {
      pipeline.clear();
      pipelineRef.current = null;
      if (idleTimerRef.current) { clearTimeout(idleTimerRef.current); idleTimerRef.current = null; }
    };
  }, []);

  // Reset on items change (scope navigation, sort, search — any new result set)
  useEffect(() => {
    firstPaintRef.current = false;
    // Restore scroll position from back/forward navigation
    if (initialScrollTop != null && initialScrollTop > 0 && containerRef.current) {
      containerRef.current.scrollTop = initialScrollTop;
    }
  }, [items]); // eslint-disable-line react-hooks/exhaustive-deps -- initialScrollTop read once per items change

  // ── Draw functions ──
  const drawBase = useCallback(() => {
    const container = containerRef.current;
    const canvas = baseCanvasRef.current;
    const pipeline = pipelineRef.current;
    if (!container || !canvas || !pipeline) return;

    const vp = snapshotViewport(container);
    const ctx = canvas.getContext('2d', { desynchronized: true });
    if (!ctx) return;

    ensureCanvasSize(canvas, vp.containerWidth, vp.viewportHeight, vp.dpr);

    const scrollTop = interactive ? vp.scrollTop : frozenScrollTop;
    const now = performance.now();

    ctx.save();
    ctx.scale(vp.dpr, vp.dpr);
    ctx.clearRect(0, 0, vp.containerWidth, vp.viewportHeight);

    // Single activation zone: viewport ± 100px.
    // Tiles inside: drawn, loaded, fade-animated.
    // Tiles outside: loads cancelled, not drawn.
    const ACTIVATION_MARGIN = 100;
    const zoneTop = scrollTop - ACTIVATION_MARGIN;
    const zoneBottom = scrollTop + vp.viewportHeight + ACTIVATION_MARGIN;
    const activeTiles: number[] = [];

    // Full scan — masonry positions are NOT Y-sorted (tiles placed in shortest column).
    const visibleHashes = new Set<string>();
    for (let i = 0; i < layout.positions.length; i++) {
      const pos = layout.positions[i];
      if (!pos) continue;

      const inZone = pos.y + pos.h >= zoneTop && pos.y <= zoneBottom;
      if (!inZone) continue;

      activeTiles.push(i);
      const item = renderItems[i];
      if (!item) continue;

      const entry = pipeline.get(item.thumbnailHash);
      if (entry?.thumb != null) {
        visibleHashes.add(item.thumbnailHash);
      }

      const imageHeight = pos.h - textHeight;
      pipeline.ensure(item.thumbnailHash, {
        y: pos.y + pos.h / 2,
        drawWidth: pos.w,
        drawHeight: imageHeight,
      });
    }

    // ── Tile reveal — read directly from pipeline entry (legacy approach) ──
    // animateIn + revealStartedAt live on the pipeline entry.
    // Fresh bitmap load → animateIn=true, revealStartedAt=perf.now().
    // Eviction → animateIn=false. Re-load → fresh timestamp → re-fades.
    const revealProgressByHash = new Map<string, number>();
    let hasActiveReveal = false;
    for (const hash of visibleHashes) {
      const entry = pipeline.get(hash);
      if (!entry?.thumb) { revealProgressByHash.set(hash, 0); continue; }
      // If entry was reset (scrolled out then back), re-trigger fade
      if (!entry.animateIn) {
        entry.animateIn = true;
        entry.revealStartedAt = now;
      }
      const elapsed = Math.max(0, now - entry.revealStartedAt);
      const progress = Math.min(1, elapsed / REVEAL_DURATION_MS);
      revealProgressByHash.set(hash, progress);
      if (progress < 1) hasActiveReveal = true;
    }

    const drawCtx: DrawContext = {
      scrollTop,
      viewportHeight: vp.viewportHeight,
      textHeight,
      borderRadius: 4,
    };

    const hasActiveRevealFromDraw = drawCanvasBaseLayer({
      ctx,
      positions: layout.positions,
      items: renderItems,
      atlasGet: (hash) => pipeline.get(hash),
      revealProgressByHash,
      activeTiles,
      draw: drawCtx,
      theme: {
        placeholderBg: 'rgba(255, 255, 255, 0.04)',
        borderRadius: 4,
        textPrimary: 'rgba(255, 255, 255, 0.92)',
        textTertiary: 'rgba(255, 255, 255, 0.36)',
      },
      viewMode: effectiveViewMode,
      fitThumbnails,
      showTileName: showName,
      showResolution,
      showExtension,
      showExtensionLabel: showExtension,
    });
    hasActiveReveal = hasActiveReveal || hasActiveRevealFromDraw;

    ctx.restore();

    // Cancel queued/in-flight loads outside the zone + reset reveal for off-screen entries
    pipeline.cancelOutsideWindow(zoneTop, zoneBottom);
    pipeline.resetRevealOutsideWindow(visibleHashes);

    // Continue animation loop for active reveals
    if (hasActiveReveal) {
      markDirty('base');
    }

    // First paint notification
    if (!firstPaintRef.current && activeTiles.length > 0) {
      firstPaintRef.current = true;
      onFirstPaint?.();
    }
  }, [layout, renderItems, effectiveViewMode, fitThumbnails, showName, showExtension, showResolution, suppressTileReveal, textHeight, interactive, frozenScrollTop]);

  const drawOverlay = useCallback(() => {
    const container = containerRef.current;
    const canvas = overlayCanvasRef.current;
    if (!container || !canvas) return;

    const vp = snapshotViewport(container);
    const ctx = canvas.getContext('2d', { desynchronized: true });
    if (!ctx) return;

    ensureCanvasSize(canvas, vp.containerWidth, vp.viewportHeight, vp.dpr);
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    ctx.save();
    ctx.scale(vp.dpr, vp.dpr);
    const scrollTop = interactive ? vp.scrollTop : frozenScrollTop;

    // Draw selection borders on all selected tiles
    if (selectedEntityHashes.size > 0) {
      ctx.strokeStyle = '#3297FF';
      ctx.lineWidth = 2;
      ctx.beginPath();
      for (let i = 0; i < items.length; i++) {
        if (!selectedEntityHashes.has(items[i].entity_hash)) continue;
        const pos = layout.positions[i];
        if (!pos) continue;
        const drawY = pos.y - scrollTop;
        if (drawY + pos.h < -100 || drawY > vp.viewportHeight + 100) continue;
        const imgH = pos.h - textHeight;
        ctx.roundRect(pos.x - 1, drawY - 1, pos.w + 2, imgH + 2, 4);
      }
      ctx.stroke();
    }

    // Draw hover zoom button (bottom-right of hovered tile)
    const hovIdx = hoveredTileRef.current;
    if (hovIdx != null && !isScrollingRef.current) {
      const hovItem = items[hovIdx];
      const hovPos = layout.positions[hovIdx];
      if (hovItem && hovPos && hovItem.entity_kind !== 'collection' && !hovItem.mime_type.startsWith('video/')) {
        const drawY = hovPos.y - scrollTop;
        if (drawY + hovPos.h >= 0 && drawY <= vp.viewportHeight) {
          const imgH = hovPos.h - textHeight;
          const ZOOM_SIZE = 24;
          const bgW = ZOOM_SIZE + 4;
          const bgH = ZOOM_SIZE + 2;
          const zx = hovPos.x + hovPos.w - bgW;
          const zy = drawY + imgH - bgH;

          // Background pill
          ctx.fillStyle = 'rgba(0, 0, 0, 0.4)';
          ctx.beginPath();
          ctx.roundRect(zx, zy, bgW, bgH, [10, 0, 4, 0]);
          ctx.fill();

          // Magnifying glass
          const cx = zx + bgW / 2;
          const cy = zy + bgH / 2;
          ctx.strokeStyle = 'rgba(255, 255, 255, 0.7)';
          ctx.lineWidth = 1.5;
          ctx.beginPath();
          ctx.arc(cx, cy, 5, 0, Math.PI * 2);
          ctx.stroke();
          // Handle
          ctx.beginPath();
          ctx.moveTo(cx + 3.5, cy + 3.5);
          ctx.lineTo(cx + 6, cy + 6);
          ctx.stroke();
          // Plus crosshair
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(cx, cy - 2.5);
          ctx.lineTo(cx, cy + 2.5);
          ctx.stroke();
          ctx.beginPath();
          ctx.moveTo(cx - 2.5, cy);
          ctx.lineTo(cx + 2.5, cy);
          ctx.stroke();
        }
      }
    }

    // Draw marquee rectangle
    const mRect = marqueeRectRef.current;
    if (mRect) {
      const mx = mRect.left;
      const my = mRect.top - scrollTop;
      ctx.fillStyle = 'rgba(51, 154, 240, 0.12)';
      ctx.fillRect(mx, my, mRect.width, mRect.height);
      ctx.strokeStyle = 'rgba(51, 154, 240, 0.5)';
      ctx.lineWidth = 1;
      ctx.strokeRect(mx + 0.5, my + 0.5, mRect.width - 1, mRect.height - 1);
    }

    ctx.restore();
  }, [layout, items, selectedEntityHashes, textHeight, interactive, frozenScrollTop]);

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
  useEffect(() => { markDirty('base'); }, [showName, showExtension, showResolution, effectiveViewMode, fitThumbnails, suppressTileReveal, markDirty]);
  useEffect(() => { markDirty('overlay'); }, [selectedEntityHashes, markDirty]);


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
    isScrollingRef.current = nextScrollState.phase !== 'idle';
    pipelineRef.current?.setScrollState(nextScrollState);

    // Clear hover and preview during scroll
    if (hoveredTileRef.current != null) {
      hoveredTileRef.current = null;
    }
    if (hoverTimerRef.current) { clearTimeout(hoverTimerRef.current); hoverTimerRef.current = null; }
    if (hoverPreview) setHoverPreview(null);

    onScrollTopChangeRef.current?.(scrollTop);
    markDirty('both');

    // Transition to idle after inactivity
    if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
    idleTimerRef.current = setTimeout(() => {
      scrollStateRef.current = createIdleCanvasScrollState();
      isScrollingRef.current = false;
      markDirty('base');
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
    // Suppress click that fires after a marquee drag ends
    if (dragJustEndedRef.current) {
      dragJustEndedRef.current = false;
      return;
    }
    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top + container.scrollTop;

    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);
    if (idx != null && items[idx]) {
      onTileClick?.(idx, items[idx], e);
    } else {
      onEmptyClick?.();
    }
  }, [items, layout.positions, onTileClick, onEmptyClick, textHeight]);

  // ── Double-click handler ──
  const handleDoubleClick = useCallback((e: React.MouseEvent) => {
    if (!onTileDoubleClick) return;
    const container = containerRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top + container.scrollTop;
    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);
    if (idx != null && items[idx]) {
      onTileDoubleClick(idx, items[idx]);
    }
  }, [items, layout.positions, onTileDoubleClick, textHeight]);

  // ── Zoom button hit test (bottom-right corner of tile image area) ──
  const isZoomButtonHit = useCallback((clientX: number, clientY: number, tileIdx: number): boolean => {
    const container = containerRef.current;
    if (!container) return false;
    const rect = container.getBoundingClientRect();
    const mx = clientX - rect.left;
    const my = clientY - rect.top + container.scrollTop;
    const pos = layout.positions[tileIdx];
    if (!pos) return false;
    const imgH = pos.h - textHeight;
    const bgW = ZOOM_BTN_SIZE + 4;
    const bgH = ZOOM_BTN_SIZE + 2;
    const zx = pos.x + pos.w - bgW;
    const zy = pos.y + imgH - bgH;
    return mx >= zx && mx < zx + bgW && my >= zy && my < zy + bgH;
  }, [layout.positions, textHeight]);

  // ── Clear hover timers helper ──
  const clearHoverTimers = useCallback(() => {
    if (hoverTimerRef.current) { clearTimeout(hoverTimerRef.current); hoverTimerRef.current = null; }
    if (hoverHideTimerRef.current) { clearTimeout(hoverHideTimerRef.current); hoverHideTimerRef.current = null; }
  }, []);

  // ── Mouse move — hover tracking + zoom button preview logic ──
  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (marqueeRef.current.active) return;
    const container = containerRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top + container.scrollTop;
    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);

    if (idx !== hoveredTileRef.current) {
      hoveredTileRef.current = idx;
      markDirty('overlay');
    }

    // Hover preview: triggered when cursor is over the zoom button area
    if (idx != null && isZoomButtonHit(e.clientX, e.clientY, idx)) {
      const item = items[idx];
      const isPreviewable = item && !item.mime_type.startsWith('video/') && item.entity_kind !== 'collection';

      // Cancel any pending hide
      if (hoverHideTimerRef.current) {
        clearTimeout(hoverHideTimerRef.current);
        hoverHideTimerRef.current = null;
      }

      if (isPreviewable && !hoverTimerRef.current) {
        // Start preview timer
        hoverTimerRef.current = setTimeout(() => {
          hoverTimerRef.current = null;
          if (item) {
            setHoverPreview((prev) =>
              prev?.hash === item.entity_hash ? prev : { hash: item.entity_hash, mime: item.mime_type },
            );
          }
        }, HOVER_PREVIEW_DELAY_MS);
      }
    } else {
      // Cursor not on zoom button — cancel show timer, start hide timer
      if (hoverTimerRef.current) {
        clearTimeout(hoverTimerRef.current);
        hoverTimerRef.current = null;
      }
      if (hoverPreview && !hoverHideTimerRef.current) {
        hoverHideTimerRef.current = setTimeout(() => {
          hoverHideTimerRef.current = null;
          setHoverPreview(null);
        }, HOVER_HIDE_DELAY_MS);
      }
    }
  }, [layout.positions, textHeight, items, isZoomButtonHit, hoverPreview, markDirty]);

  const handleMouseLeave = useCallback(() => {
    if (hoveredTileRef.current != null) {
      hoveredTileRef.current = null;
      markDirty('overlay');
    }
    clearHoverTimers();
    if (hoverPreview) {
      hoverHideTimerRef.current = setTimeout(() => {
        hoverHideTimerRef.current = null;
        setHoverPreview(null);
      }, HOVER_HIDE_DELAY_MS);
    }
  }, [clearHoverTimers, hoverPreview, markDirty]);

  // ── Context menu handler ──
  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top + container.scrollTop;
    const pos = { x: e.clientX, y: e.clientY };

    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);
    if (idx != null && items[idx]) {
      onTileContextMenu?.(idx, items[idx], pos);
    } else {
      onEmptyContextMenu?.(pos);
    }
  }, [items, layout.positions, onTileContextMenu, onEmptyContextMenu, textHeight]);

  // ── Marquee drag handlers ──
  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return; // left button only
    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top + container.scrollTop;

    // Only start marquee if clicking empty space (not on a tile)
    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);
    if (idx != null) return;

    marqueeRef.current = { startX: x, startY: y, active: true, shiftKey: e.shiftKey, lastClientX: x, lastClientY: e.clientY - container.getBoundingClientRect().top };
    marqueeBaseSelectionRef.current = e.shiftKey || e.metaKey || e.ctrlKey
      ? new Set(selectedEntityHashes)
      : new Set();
    marqueeRectRef.current = null;
    autoScrollSpeedRef.current = 0;
    container.setPointerCapture(e.pointerId);

    // Start auto-scroll RAF loop — also updates marquee rect during scroll
    if (autoScrollRef.current == null) {
      const tick = () => {
        if (!marqueeRef.current.active) { autoScrollRef.current = null; return; }
        const c = containerRef.current;
        if (c && autoScrollSpeedRef.current !== 0) {
          c.scrollTop += autoScrollSpeedRef.current;
          // Update marquee rect using stored cursor position + new scroll
          const { startX: sx, startY: sy, lastClientX: lcx, lastClientY: lcy } = marqueeRef.current;
          const curY = lcy + c.scrollTop;
          const curX = lcx;
          const l = Math.min(sx, curX);
          const t = Math.min(sy, curY);
          const w = Math.abs(curX - sx);
          const h = Math.abs(curY - sy);
          if (w >= 5 || h >= 5) {
            marqueeRectRef.current = { left: l, top: t, width: w, height: h };
            // Re-hit-test tiles
            const hitH = new Set(marqueeBaseSelectionRef.current);
            const positions = layout.positions;
            for (let i = 0; i < positions.length; i++) {
              const pos = positions[i];
              if (!pos || !items[i]) continue;
              const imgH = pos.h - textHeight;
              if (pos.x + pos.w > l && pos.x < l + w && pos.y + imgH > t && pos.y < t + h) {
                hitH.add(items[i].entity_hash);
              }
            }
            onSelectionChange?.(hitH);
          }
          markDirty('both');
        }
        autoScrollRef.current = requestAnimationFrame(tick);
      };
      autoScrollRef.current = requestAnimationFrame(tick);
    }
  }, [layout.positions, textHeight, selectedEntityHashes]);

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    if (!marqueeRef.current.active) return;
    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    const clientY = e.clientY - rect.top;
    const x = e.clientX - rect.left;
    const y = clientY + container.scrollTop;
    marqueeRef.current.lastClientX = e.clientX - rect.left;
    marqueeRef.current.lastClientY = clientY;
    const { startX, startY } = marqueeRef.current;

    // Auto-scroll: set speed based on cursor proximity to edge.
    // A RAF loop in the background applies the scroll continuously.
    const EDGE_ZONE = 50;
    const MAX_SPEED = 12;
    if (clientY < EDGE_ZONE) {
      autoScrollSpeedRef.current = -MAX_SPEED * (1 - clientY / EDGE_ZONE);
    } else if (clientY > container.clientHeight - EDGE_ZONE) {
      autoScrollSpeedRef.current = MAX_SPEED * (1 - (container.clientHeight - clientY) / EDGE_ZONE);
    } else {
      autoScrollSpeedRef.current = 0;
    }

    const left = Math.min(startX, x);
    const top = Math.min(startY, y);
    const width = Math.abs(x - startX);
    const height = Math.abs(y - startY);

    if (width < 5 && height < 5) return;

    marqueeRectRef.current = { left, top, width, height };

    // Compute intersecting tiles
    const hitHashes = new Set(marqueeBaseSelectionRef.current);
    for (let i = 0; i < layout.positions.length; i++) {
      const pos = layout.positions[i];
      if (!pos || !items[i]) continue;
      const imgH = pos.h - textHeight;
      if (pos.x + pos.w > left && pos.x < left + width &&
          pos.y + imgH > top && pos.y < top + height) {
        hitHashes.add(items[i].entity_hash);
      }
    }
    onSelectionChange?.(hitHashes);
    markDirty('overlay');
  }, [items, layout.positions, textHeight, onSelectionChange, markDirty]);

  const handlePointerUp = useCallback((e: React.PointerEvent) => {
    if (!marqueeRef.current.active) return;
    const hadVisibleMarquee = marqueeRectRef.current != null;
    marqueeRef.current.active = false;
    marqueeRectRef.current = null;
    autoScrollSpeedRef.current = 0;
    if (autoScrollRef.current != null) {
      cancelAnimationFrame(autoScrollRef.current);
      autoScrollRef.current = null;
    }
    const container = containerRef.current;
    if (container) container.releasePointerCapture(e.pointerId);
    if (hadVisibleMarquee) dragJustEndedRef.current = true;
    markDirty('overlay');
  }, [markDirty]);

  // ── Render ──
  return (
    <div className={styles.root}>
      <div
        ref={containerRef}
        className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}
        onScroll={handleScroll}
        onClick={handleClick}
        onDoubleClick={handleDoubleClick}
        onContextMenu={handleContextMenu}
        onMouseMove={handleMouseMove}
        onMouseLeave={handleMouseLeave}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
      >
        <div
          className={styles.canvasWrap}
          style={{ height: `${estimatedTotalHeight}px` }}
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
      {hoverPreview && <HoverPreviewPortal hash={hoverPreview.hash} mime={hoverPreview.mime} />}
    </div>
  );
}
