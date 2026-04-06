/**
 * Canvas2D grid renderer — dual-canvas (base + overlay) with thumbnail pipeline.
 *
 * Activation zone: viewport ± 100px. Tiles inside are drawn, loaded, and
 * fade-animated. Tiles outside get loads cancelled and stop rendering.
 * One linear scan per frame drives everything.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode, LayoutResult } from '../layout/types';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { HoverPreviewPortal } from './HoverPreviewPortal';
import { drawCanvasBaseLayer, type DrawContext } from './drawBase';

import { ThumbnailPipeline } from './thumbnailPipeline';
import { adaptGridItem } from './renderItemAdapter';
import { startDrag, moveDrag, endDrag, cancelDrag, setDropTarget, getDragState, isDragActive } from '../dragState';
import { hitTestTile, computeReorderTarget } from './hitTesting';
import { DragGhost } from '../DragGhost';
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
const EMPTY_HASH_SET = new Set<string>();

/** Convert viewport (visual) coordinates to CSS layout coordinates.
 *  Uses shared zoom compensation for browser zoom support. */
function toLayoutCoords(clientX: number, clientY: number, container: HTMLDivElement, headerHeight: number) {
  const rect = container.getBoundingClientRect();
  const zoomX = rect.width / (container.offsetWidth || 1);
  const zoomY = rect.height / (container.offsetHeight || 1);
  return {
    x: (clientX - rect.left) / zoomX,
    y: (clientY - rect.top) / zoomY + container.scrollTop - headerHeight,
    rect,
    zoom: zoomX,
  };
}
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
  /** DOM content rendered above the canvas inside the scroll container (e.g. SubfolderGrid). */
  headerContent?: React.ReactNode;
  /** Current scope for drag-and-drop context. */
  dragSourceScope?: { kind: string; id?: number | null; key?: string | null } | null;
  /** Expose the scroll container ref to parent. */
  onContainerRef?: (el: HTMLDivElement | null) => void;
  /** Notify parent when layout changes (for scroll-to-item). */
  onLayoutChange?: (layout: LayoutResult) => void;
  /** Index of tile currently being renamed (inline edit overlay). */
  renamingIndex?: number | null;
  /** Called when inline rename commits or cancels. */
  onRenameCommit?: (index: number, newName: string) => void;
  onRenameCancel?: () => void;
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
  selectedEntityHashes = EMPTY_HASH_SET,
  headerContent,
  dragSourceScope = null,
  onContainerRef,
  onLayoutChange,
  renamingIndex = null,
  onRenameCommit,
  onRenameCancel,
}: CanvasGridProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const containerCallbackRef = useCallback((el: HTMLDivElement | null) => {
    (containerRef as React.MutableRefObject<HTMLDivElement | null>).current = el;
    onContainerRef?.(el);
  }, [onContainerRef]);
  const headerRef = useRef<HTMLDivElement>(null);
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
  const tileDragRef = useRef<{ tileIdx: number; startClientX: number; startClientY: number } | null>(null);
  const reorderDropRef = useRef<{ dropIndex: number; dropSide: 'left' | 'right' } | null>(null);
  const autoScrollRef = useRef<number | null>(null);
  const autoScrollSpeedRef = useRef(0);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [hoverPreview, setHoverPreview] = useState<{ hash: string; mime: string } | null>(null);
  const [marqueeVisual, setMarqueeVisual] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  const [dragGhost, setDragGhost] = useState<{ x: number; y: number; count: number; thumbnailHashes: string[] } | null>(null);
  const firstPaintRef = useRef(false);
  const onLoadMoreRef = useRef(onLoadMore);
  const onScrollTopChangeRef = useRef(onScrollTopChange);
  onLoadMoreRef.current = onLoadMore;
  onScrollTopChangeRef.current = onScrollTopChange;

  // Debounced container dimensions for layout — prevents jitter during resize
  const [containerWidth, setContainerWidth] = useState(0);
  const [containerHeight, setContainerHeight] = useState(0);
  const [scrollbarWidth, setScrollbarWidth] = useState(0);
  const [headerHeight, setHeaderHeight] = useState(0);
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

  const onLayoutChangeRef = useRef(onLayoutChange);
  onLayoutChangeRef.current = onLayoutChange;
  useEffect(() => { onLayoutChangeRef.current?.(layout); }, [layout]);

  // ── Scroll anchor on resize / zoom / viewMode / display toggle ──
  // Anchor on the TOP EDGE of the topmost visible tile. The image top
  // is stable when text height changes (names/resolution toggle adds
  // space below images, not above). For zoom and view mode changes
  // the tile top is still the most intuitive anchor point.
  //
  // useLayoutEffect so we run before the browser paints / clamps scrollTop.
  // lastScrollTopRef is the pre-layout scroll position (kept in sync by
  // handleScroll and the initialScrollTop restore).
  const selectedHashesRef = useRef(selectedEntityHashes);
  selectedHashesRef.current = selectedEntityHashes;

  useLayoutEffect(() => {
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = layout;
    const prevItems = prevItemsRef.current;
    prevItemsRef.current = items;

    // Items changed → scope transition handles its own scroll. Skip.
    if (prevItems !== items) return;
    // No previous layout (first mount / first resize) or same layout object.
    if (!prev || prev === layout) return;
    if (prev.positions.length === 0 || layout.positions.length === 0) return;

    const container = containerRef.current;
    if (!container) return;
    const vh = container.clientHeight;
    if (vh === 0) return;

    const scrollTop = lastScrollTopRef.current;
    const vpTop = scrollTop;
    const vpBot = scrollTop + vh;

    let anchorIdx = -1;
    let bestTop = Infinity;
    const sel = selectedHashesRef.current;

    // 1) Prefer a selected tile that is at least partially visible
    if (sel.size > 0) {
      for (let i = 0; i < prev.positions.length; i++) {
        const p = prev.positions[i];
        if (!p || !items[i] || !sel.has(items[i].entity_hash)) continue;
        if (p.y + p.h < vpTop || p.y > vpBot) continue;
        if (p.y < bestTop) { bestTop = p.y; anchorIdx = i; }
      }
    }

    // 2) Fall back to the topmost tile that overlaps the viewport
    if (anchorIdx < 0) {
      bestTop = Infinity;
      for (let i = 0; i < prev.positions.length; i++) {
        const p = prev.positions[i];
        if (!p) continue;
        if (p.y + p.h < vpTop || p.y > vpBot) continue;
        if (p.y < bestTop) { bestTop = p.y; anchorIdx = i; }
      }
    }

    if (anchorIdx < 0 || anchorIdx >= layout.positions.length) return;

    // Anchor on the tile's top edge — stable when text height changes
    const oldTop = prev.positions[anchorIdx].y;
    const newTop = layout.positions[anchorIdx].y;
    const offset = oldTop - scrollTop;
    const next = Math.max(0, newTop - offset);

    container.scrollTop = next;
    lastScrollTopRef.current = next;
  }, [layout, items]);

  // ── Estimated total scroll height ──
  // When totalCount > loaded items, estimate from average height per item.
  // First page loads 500 items — enough for a good estimate.
  // As more 100-item batches load, the estimate refines imperceptibly.
  const estimatedTotalHeight = useMemo(() => {
    const loadedCount = items.length;
    const total = totalCount ?? loadedCount;
    const mediaHeight = (total <= loadedCount || loadedCount === 0)
      ? layout.totalHeight
      : Math.round((layout.totalHeight / loadedCount) * total);
    return mediaHeight + headerHeight;
  }, [items.length, totalCount, layout.totalHeight, headerHeight]);

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

  // Track previous items length separately for the scroll reset decision.
  // prevItemsRef is updated in useLayoutEffect (runs first), so we need our own tracker.
  const prevItemsLengthForScrollRef = useRef(0);
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const prevLen = prevItemsLengthForScrollRef.current;
    prevItemsLengthForScrollRef.current = items.length;

    if (initialScrollTop != null && initialScrollTop > 0) {
      // Restore scroll position from back/forward navigation
      firstPaintRef.current = false;
      container.scrollTop = initialScrollTop;
      lastScrollTopRef.current = initialScrollTop;
    } else if (prevLen === 0 || items.length === 0) {
      // Fresh navigation or cleared → start at top
      firstPaintRef.current = false;
      container.scrollTop = 0;
      lastScrollTopRef.current = 0;
    }
    // Otherwise: items grew/changed incrementally — don't touch scroll or firstPaint
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

    const rawScrollTop = interactive ? vp.scrollTop : frozenScrollTop;
    // Offset by header height — tile positions start at Y=0 but the header pushes canvas content down
    const scrollTop = Math.max(0, rawScrollTop - headerHeight);
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
    const planTiles: Array<{ hash: string; mime: string; w: number; h: number }> = [];
    for (let i = 0; i < layout.positions.length; i++) {
      const pos = layout.positions[i];
      if (!pos) continue;

      const inZone = pos.y + pos.h >= zoneTop && pos.y <= zoneBottom;
      if (!inZone) continue;

      activeTiles.push(i);
      const item = renderItems[i];
      if (!item) continue;

      visibleHashes.add(item.thumbnailHash);
      planTiles.push({ hash: item.thumbnailHash, mime: item.mime, w: pos.w, h: pos.h });
    }

    // Send plan to worker — it handles loading, caching, staggered reveals.
    // Tiles exceeding the quality threshold get full-res URLs automatically.
    pipeline.updatePlan(planTiles);

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
      now,
      activeTiles,
      draw: drawCtx,
      theme: (() => {
        const s = getComputedStyle(container);
        return {
          placeholderBg: s.getPropertyValue('--color-surface-2').trim() || 'rgba(255,255,255,0.04)',
          borderRadius: 4,
          textPrimary: s.getPropertyValue('--color-text-primary').trim() || 'rgba(255,255,255,0.92)',
          textTertiary: s.getPropertyValue('--color-text-tertiary').trim() || 'rgba(255,255,255,0.36)',
          glassBorder: s.getPropertyValue('--color-border-primary').trim() || 'rgba(255,255,255,0.14)',
          tileBoundary: s.getPropertyValue('--color-border-secondary').trim() || 'rgba(255,255,255,0.12)',
        };
      })(),
      viewMode: effectiveViewMode,
      fitThumbnails,
      showTileName: showName,
      showResolution,
      showExtension,
      showExtensionLabel: showExtension,
    });

    ctx.restore();

    // Evict main-thread bitmaps outside the draw zone.
    // The worker handles load cancellation via the plan diff.
    pipeline.evictOutsideVisible(visibleHashes);

    // Continue animation loop for active reveals
    if (hasActiveRevealFromDraw) {
      markDirty('base');
    }

    // First paint notification
    if (!firstPaintRef.current && activeTiles.length > 0) {
      firstPaintRef.current = true;
      onFirstPaint?.();
    }
  }, [layout, renderItems, effectiveViewMode, fitThumbnails, showName, showExtension, showResolution, suppressTileReveal, textHeight, interactive, frozenScrollTop, headerHeight]);

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
    const scrollTop = Math.max(0, (interactive ? vp.scrollTop : frozenScrollTop) - headerHeight);

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

    // Marquee is rendered as a DOM overlay (covers both header and canvas)

    // Draw reorder drop indicator (blue vertical line + triangle)
    const rd = reorderDropRef.current;
    if (rd) {
      const rdPos = layout.positions[rd.dropIndex];
      if (rdPos) {
        const rdDrawY = rdPos.y - scrollTop;
        const rdImgH = rdPos.h - textHeight;
        const gap = 16;
        const indicatorX = rd.dropSide === 'left'
          ? rdPos.x - gap / 2
          : rdPos.x + rdPos.w + gap / 2;

        // Vertical line
        ctx.strokeStyle = '#228be6';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(indicatorX, rdDrawY);
        ctx.lineTo(indicatorX, rdDrawY + rdImgH);
        ctx.stroke();

        // Triangle arrow at top
        ctx.fillStyle = '#228be6';
        ctx.beginPath();
        ctx.moveTo(indicatorX - 5, rdDrawY);
        ctx.lineTo(indicatorX + 5, rdDrawY);
        ctx.lineTo(indicatorX, rdDrawY + 7);
        ctx.closePath();
        ctx.fill();
      }
    }

    ctx.restore();
  }, [layout, items, selectedEntityHashes, textHeight, interactive, frozenScrollTop, headerHeight]);

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

    // Re-measure when page zoom changes (Cmd+/Cmd-) — DPR changes with zoom
    const dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    const handleDprChange = () => measure();
    dprQuery.addEventListener('change', handleDprChange);

    return () => {
      observer.disconnect();
      dprQuery.removeEventListener('change', handleDprChange);
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
    };
  }, []);

  // ── Header height observer ──
  useEffect(() => {
    const el = headerRef.current;
    if (!el) { setHeaderHeight(0); return; }
    const measure = () => setHeaderHeight(el.offsetHeight);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [headerContent]);

  // ── Redraw on layout/prop changes ──
  useEffect(() => { markDirty('both'); }, [layout, markDirty]);
  useEffect(() => { markDirty('base'); }, [showName, showExtension, showResolution, effectiveViewMode, fitThumbnails, suppressTileReveal, markDirty]);
  useEffect(() => { markDirty('overlay'); }, [selectedEntityHashes, markDirty]);
  useEffect(() => { markDirty('both'); }, [headerHeight, markDirty]);

  // ── Redraw on theme change (canvas reads CSS variables, not reactive to theme) ──
  useEffect(() => {
    const observer = new MutationObserver(() => markDirty('both'));
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme', 'data-mantine-color-scheme', 'style'],
    });
    return () => observer.disconnect();
  }, [markDirty]);

  // ── Global pointer tracking during drag ──
  // Uses refs to avoid effect re-runs on every ghost position update.
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const layoutPosRef = useRef(layout.positions);
  layoutPosRef.current = layout.positions;
  const textHeightRef = useRef(textHeight);
  textHeightRef.current = textHeight;
  const headerHeightRef = useRef(headerHeight);
  headerHeightRef.current = headerHeight;
  const markDirtyRef = useRef(markDirty);
  markDirtyRef.current = markDirty;
  const onSelectionChangeRef = useRef(onSelectionChange);
  onSelectionChangeRef.current = onSelectionChange;

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!isDragActive()) return;
      moveDrag(e.clientX, e.clientY);
      setDragGhost((prev) => prev ? { ...prev, x: e.clientX, y: e.clientY } : null);
      const scope = getDragState().sourceScope;
      if (scope && scope.kind === 'folder') {
        const ctr = containerRef.current;
        if (ctr) {
          const { x: cx, y: cy } = toLayoutCoords(e.clientX, e.clientY, ctr, headerHeightRef.current);
          // Build skip set from dragged item indices
          const draggedHashes = new Set(getDragState().hashes);
          const skipIdx = new Set<number>();
          const curItems = itemsRef.current;
          for (let i = 0; i < curItems.length; i++) {
            if (draggedHashes.has(curItems[i].entity_hash)) skipIdx.add(i);
          }
          const tgt = computeReorderTarget(layoutPosRef.current, cx, cy, textHeightRef.current, skipIdx);
          reorderDropRef.current = tgt ? { dropIndex: tgt.index, dropSide: tgt.side } : null;
          markDirtyRef.current('overlay');
        }
      }
    };
    const onUp = () => {
      if (isDragActive()) {
        const rd = reorderDropRef.current;
        const existingTarget = getDragState().dropTarget;
        if (rd && !existingTarget) {
          const curItems = itemsRef.current;
          const draggedSet = new Set(getDragState().hashes);
          const targetIdx = rd.dropSide === 'right' ? rd.dropIndex + 1 : rd.dropIndex;
          let offset = 0;
          for (let i = 0; i < targetIdx && i < curItems.length; i++) {
            if (draggedSet.has(curItems[i].entity_hash)) offset++;
          }
          const insertAt = targetIdx - offset;
          const dragged = curItems.filter((it) => draggedSet.has(it.entity_hash));
          const remaining = curItems.filter((it) => !draggedSet.has(it.entity_hash));
          const reordered = [...remaining.slice(0, insertAt), ...dragged, ...remaining.slice(insertAt)];
          const orderedEntityIds: [number, number][] = reordered.map((it, i) => [it.entity_id, i]);
          if (orderedEntityIds.length > 0) {
            setDropTarget({ kind: 'reorder', orderedEntityIds });
          }
        }
        reorderDropRef.current = null;
        // Preserve selection: re-select the dragged hashes after drop
        const draggedHashes = new Set(getDragState().hashes);
        endDrag();
        dragJustEndedRef.current = true; // suppress the click that follows mouseup
        onSelectionChangeRef.current?.(draggedHashes);
        markDirtyRef.current('overlay');
      }
      setDragGhost(null);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      reorderDropRef.current = null;
      if (isDragActive()) cancelDrag();
    };
  }, []); // stable — never re-runs, uses refs for all mutable data


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
  const isInHeader = useCallback((target: EventTarget) => {
    return headerRef.current?.contains(target as Node) ?? false;
  }, []);

  /** Check if the event target is on an interactive element inside the header (folder tile, button, etc.) */
  const isOnHeaderInteractive = useCallback((target: EventTarget) => {
    const el = target as HTMLElement;
    if (!headerRef.current?.contains(el)) return false;
    // Check if the click is on a folder tile or its children
    return !!el.closest('[data-folder-hash]') || !!el.closest('button');
  }, []);

  const handleClick = useCallback((e: React.MouseEvent) => {
    // Suppress click that fires after a marquee drag ends
    if (dragJustEndedRef.current) {
      dragJustEndedRef.current = false;
      return;
    }
    // Folder tiles handle their own clicks; empty header space clears selection
    if (isOnHeaderInteractive(e.target)) return;

    const container = containerRef.current;
    if (!container) return;

    const { x, y } = toLayoutCoords(e.clientX, e.clientY, container, headerHeight);
    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);

    if (idx != null && items[idx]) {
      onTileClick?.(idx, items[idx], e);
    } else {
      onEmptyClick?.();
    }
  }, [items, layout.positions, onTileClick, onEmptyClick, textHeight, isInHeader, headerHeight]);

  // ── Double-click handler ──
  const handleDoubleClick = useCallback((e: React.MouseEvent) => {
    if (isOnHeaderInteractive(e.target)) return;
    if (!onTileDoubleClick) return;
    const container = containerRef.current;
    if (!container) return;
    const { x, y } = toLayoutCoords(e.clientX, e.clientY, container, headerHeight);
    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);
    if (idx != null && items[idx]) {
      onTileDoubleClick(idx, items[idx]);
    }
  }, [items, layout.positions, onTileDoubleClick, textHeight, isInHeader, headerHeight]);

  // ── Zoom button hit test (bottom-right corner of tile image area) ──
  const isZoomButtonHit = useCallback((clientX: number, clientY: number, tileIdx: number): boolean => {
    const container = containerRef.current;
    if (!container) return false;
    const { x: mx, y: my } = toLayoutCoords(clientX, clientY, container, headerHeight);
    const pos = layout.positions[tileIdx];
    if (!pos) return false;
    const imgH = pos.h - textHeight;
    const bgW = ZOOM_BTN_SIZE + 4;
    const bgH = ZOOM_BTN_SIZE + 2;
    const zx = pos.x + pos.w - bgW;
    const zy = pos.y + imgH - bgH;
    return mx >= zx && mx < zx + bgW && my >= zy && my < zy + bgH;
  }, [layout.positions, textHeight, headerHeight]);

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
    const { x, y } = toLayoutCoords(e.clientX, e.clientY, container, headerHeight);
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
  }, [layout.positions, textHeight, items, isZoomButtonHit, hoverPreview, markDirty, headerHeight]);

  const handleMouseLeave = useCallback(() => {
    // Don't cancel drag when cursor moves to sidebar — ghost follows globally
    if (isDragActive()) return;
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
    if (isInHeader(e.target)) return; // header handles its own context menu
    e.preventDefault();
    const container = containerRef.current;
    if (!container) return;

    const { x, y } = toLayoutCoords(e.clientX, e.clientY, container, headerHeight);
    const pos = { x: e.clientX, y: e.clientY };

    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);
    if (idx != null && items[idx]) {
      onTileContextMenu?.(idx, items[idx], pos);
    } else {
      onEmptyContextMenu?.(pos);
    }
  }, [items, layout.positions, onTileContextMenu, onEmptyContextMenu, textHeight, headerHeight]);

  // ── Folder tile marquee hit-testing ──
  const hitTestFolderTiles = useCallback((left: number, top: number, width: number, height: number, hitSet: Set<string>) => {
    const header = headerRef.current;
    if (!header) return;
    const tiles = header.querySelectorAll<HTMLElement>('[data-folder-hash]');
    const container = containerRef.current;
    if (!container) return;
    for (const tile of tiles) {
      const hash = tile.dataset.folderHash;
      if (!hash) continue;
      // Tile offset in scroll-space → convert to content-space by subtracting headerHeight
      const tileTop = tile.offsetTop - headerHeight;
      const tileLeft = tile.offsetLeft;
      const tileW = tile.offsetWidth;
      const tileH = tile.offsetHeight;
      if (tileLeft + tileW > left && tileLeft < left + width &&
          tileTop + tileH > top && tileTop < top + height) {
        hitSet.add(hash);
      }
    }
  }, [headerHeight]);

  // ── Marquee drag handlers ──
  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return; // left button only
    if (isOnHeaderInteractive(e.target)) return; // folder tiles handle their own clicks
    const container = containerRef.current;
    if (!container) return;

    const { x, y } = toLayoutCoords(e.clientX, e.clientY, container, headerHeight);

    // If clicking on a tile, set up potential tile drag (not marquee)
    const idx = hitTestTile(layout.positions, x, y, textHeight, 0, layout.positions.length);
    if (idx != null) {
      tileDragRef.current = { tileIdx: idx, startClientX: e.clientX, startClientY: e.clientY };
      return;
    }
    tileDragRef.current = null;

    const mRect = container.getBoundingClientRect();
    const mZoomY = mRect.height / (container.offsetHeight || 1);
    marqueeRef.current = { startX: x, startY: y, active: true, shiftKey: e.shiftKey, lastClientX: x, lastClientY: (e.clientY - mRect.top) / mZoomY };
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
          const curY = lcy + c.scrollTop - headerHeight;
          const curX = lcx;
          const l = Math.min(sx, curX);
          const t = Math.min(sy, curY);
          const w = Math.abs(curX - sx);
          const h = Math.abs(curY - sy);
          if (w >= 5 || h >= 5) {
            marqueeRectRef.current = { left: l, top: t, width: w, height: h };
            setMarqueeVisual({ left: l, top: t + headerHeight, width: w, height: h });
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
            hitTestFolderTiles(l, t, w, h, hitH);
            onSelectionChange?.(hitH);
          }
          markDirty('both');
        }
        autoScrollRef.current = requestAnimationFrame(tick);
      };
      autoScrollRef.current = requestAnimationFrame(tick);
    }
  }, [layout.positions, textHeight, selectedEntityHashes, headerHeight]);

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    // Check for tile drag initiation (5px threshold)
    if (tileDragRef.current && !isDragActive()) {
      const dx = e.clientX - tileDragRef.current.startClientX;
      const dy = e.clientY - tileDragRef.current.startClientY;
      if (Math.abs(dx) > 5 || Math.abs(dy) > 5) {
        const tileIdx = tileDragRef.current.tileIdx;
        const item = items[tileIdx];
        if (item) {
          const hash = item.entity_hash;
          const currentSelection = selectedHashesRef.current;
          const hashes = currentSelection.has(hash)
            ? [...currentSelection]
            : [hash];
          const thumbHashes = hashes.slice(0, 3).map((h) => {
            const it = items.find((i) => i.entity_hash === h);
            return it?.thumbnail_hash ?? h;
          });
          startDrag(hashes, e.clientX, e.clientY, dragSourceScope);
          setDragGhost({ x: e.clientX, y: e.clientY, count: hashes.length, thumbnailHashes: thumbHashes });
          reorderDropRef.current = null;
          tileDragRef.current = null;
        }
      }
      return;
    }
    // Active tile drag — global onMove handler computes reorder target + ghost position
    if (isDragActive()) return;
    if (!marqueeRef.current.active) return;
    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();
    const zoomX = rect.width / (container.offsetWidth || 1);
    const zoomY = rect.height / (container.offsetHeight || 1);
    const clientY = (e.clientY - rect.top) / zoomY;
    const x = (e.clientX - rect.left) / zoomX;
    const y = clientY + container.scrollTop - headerHeight;
    marqueeRef.current.lastClientX = x;
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
    // Visual marquee in scroll-space (add headerHeight back for DOM overlay)
    setMarqueeVisual({ left, top: top + headerHeight, width, height });

    // Compute intersecting tiles (canvas + folder DOM)
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
    hitTestFolderTiles(left, top, width, height, hitHashes);
    onSelectionChange?.(hitHashes);
    markDirty('overlay');
  }, [items, layout.positions, textHeight, onSelectionChange, markDirty, hitTestFolderTiles, headerHeight]);

  const handlePointerUp = useCallback((e: React.PointerEvent) => {
    tileDragRef.current = null;
    // Tile drag end is handled by the global window mouseup listener — don't interfere here
    if (isDragActive()) return;
    if (!marqueeRef.current.active) return;
    const hadVisibleMarquee = marqueeRectRef.current != null;
    marqueeRef.current.active = false;
    marqueeRectRef.current = null;
    setMarqueeVisual(null);
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
        ref={containerCallbackRef}
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
        {headerContent && <div ref={headerRef}>{headerContent}</div>}
        {marqueeVisual && (
          <div style={{
            position: 'absolute',
            left: marqueeVisual.left,
            top: marqueeVisual.top,
            width: marqueeVisual.width,
            height: marqueeVisual.height,
            background: 'rgba(51, 154, 240, 0.12)',
            border: '1px solid rgba(51, 154, 240, 0.5)',
            pointerEvents: 'none',
            zIndex: 100,
          }} />
        )}
        {renamingIndex != null && (() => {
          const pos = layout.positions[renamingIndex];
          const item = items[renamingIndex];
          if (!pos || !item) return null;
          const imageH = pos.h - textHeight;
          return (
            <input
              key={`rename-${renamingIndex}`}
              autoFocus
              defaultValue={item.name ?? ''}
              style={{
                position: 'absolute',
                left: pos.x,
                top: pos.y + imageH + headerHeight,
                width: pos.w,
                height: textHeight,
                zIndex: 200,
                background: 'var(--color-surface-1)',
                border: '1px solid var(--color-primary)',
                borderRadius: 4,
                color: 'var(--color-text-primary)',
                fontSize: 13,
                fontFamily: 'var(--font-family)',
                textAlign: 'center',
                outline: 'none',
                padding: '0 4px',
                boxSizing: 'border-box',
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  onRenameCommit?.(renamingIndex!, (e.target as HTMLInputElement).value.trim());
                } else if (e.key === 'Escape') {
                  e.preventDefault();
                  onRenameCancel?.();
                }
              }}
              onBlur={(e) => {
                const val = e.target.value.trim();
                if (val && val !== (item.name ?? '')) {
                  onRenameCommit?.(renamingIndex!, val);
                } else {
                  onRenameCancel?.();
                }
              }}
            />
          );
        })()}
        <div
          className={styles.canvasWrap}
          style={{ height: `${estimatedTotalHeight - headerHeight}px` }}
        >
          <div
            className={styles.viewportLayer}
            style={{ height: `${Math.min(containerHeight, estimatedTotalHeight - headerHeight)}px` }}
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
      {dragGhost && (
        <DragGhost
          x={dragGhost.x}
          y={dragGhost.y}
          thumbnailHashes={dragGhost.thumbnailHashes}
          count={dragGhost.count}
        />
      )}
    </div>
  );
}
