/**
 * Canvas2D grid renderer — dual-canvas (base + overlay) with thumbnail pipeline.
 *
 * Activation zone: viewport ± 100px. Tiles inside are drawn and loaded;
 * reveal eligibility is tracked against the actual viewport.
 * One linear scan per frame drives everything.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode, LayoutResult } from '../layout/types';
import { HoverPreviewPortal } from './HoverPreviewPortal';
import { drawCanvasBaseLayer, type DrawContext } from './drawBase';

import { ThumbnailPipeline, type PlanTile } from './thumbnailPipeline';
import { ThumbnailRevealTracker } from './thumbnailRevealTracker';
import { zoomController } from '../../../controllers/zoomController';
import { collectThumbnailActivation, GridLayoutRuntime } from './gridLayoutModel';
import { startDrag, moveDrag, endDrag, cancelDrag, setDropTarget, getDragState, isDragActive, startNativeDrag as startNativeDragFn, setInternalDragOrigin } from '../dragState';
import { hitTestTile, computeReorderTarget } from './hitTesting';
import { DragGhost } from '../DragGhost';
import { createNativeDragImageUrl } from '../dragGhostSpec';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
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
import type { ItemScope } from '../../../shared/types/generated/application/ItemScope';

const GAP = 16;
const TEXT_NAME_ROW_H = 20;
const EMPTY_ITEM_ID_SET = new Set<number>();
const EMPTY_FOLDER_NODE_SET = new Set<string>();

function resolveGridScrollAnchor(args: {
  previousPositions: ReturnType<GridLayoutRuntime['update']>['positions'];
  nextPositions: ReturnType<GridLayoutRuntime['update']>['positions'];
  previousItems: readonly CanonicalEntityGridItem[];
  nextItems: readonly CanonicalEntityGridItem[];
  selectedItemIds: ReadonlySet<number>;
  scrollTop: number;
  viewportHeight: number;
}): number | null {
  const { previousPositions, nextPositions, previousItems, nextItems, selectedItemIds, scrollTop, viewportHeight } = args;
  const viewportBottom = scrollTop + viewportHeight;
  let anchorIndex = -1;
  let bestTop = Infinity;
  const findAnchor = (selected: boolean) => {
    for (let index = 0; index < previousPositions.length; index++) {
      const position = previousPositions[index];
      const item = previousItems[index];
      if (!position || (selected && (!item || !selectedItemIds.has(item.item_id)))) continue;
      if (position.y + position.h < scrollTop || position.y > viewportBottom) continue;
      if (position.y < bestTop) { bestTop = position.y; anchorIndex = index; }
    }
  };
  if (selectedItemIds.size > 0) findAnchor(true);
  if (anchorIndex < 0) { bestTop = Infinity; findAnchor(false); }
  const anchorId = previousItems[anchorIndex]?.item_id;
  if (anchorId == null) return null;
  const nextIndex = nextItems.findIndex((item) => item.item_id === anchorId);
  const previousPosition = previousPositions[anchorIndex];
  const nextPosition = nextPositions[nextIndex];
  if (!previousPosition || !nextPosition) return null;
  return Math.max(0, nextPosition.y - (previousPosition.y - scrollTop));
}

function planFolderReorder(
  orderedItemIds: number[],
  draggedItemIds: ReadonlySet<number>,
  dropIndex: number,
  dropSide: 'left' | 'right',
): number[] {
  const dragged = orderedItemIds.filter((itemId) => draggedItemIds.has(itemId));
  if (dragged.length === 0) return [];
  const targetIndex = dropSide === 'right' ? dropIndex + 1 : dropIndex;
  const draggedBeforeTarget = orderedItemIds.slice(0, targetIndex).filter((itemId) => draggedItemIds.has(itemId)).length;
  const remaining = orderedItemIds.filter((itemId) => !draggedItemIds.has(itemId));
  const insertAt = Math.max(0, Math.min(remaining.length, targetIndex - draggedBeforeTarget));
  const reordered = [...remaining.slice(0, insertAt), ...dragged, ...remaining.slice(insertAt)];
  return reordered.every((itemId, index) => itemId === orderedItemIds[index]) ? [] : reordered;
}

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
const ZOOM_BTN_SIZE = 24;
const HOVER_PREVIEW_DELAY_MS = 200;
const HOVER_HIDE_DELAY_MS = 90;
const GRID_RESIZE_SETTLE_MS = 180;

export interface CanvasGridProps {
  items: CanonicalEntityGridItem[];
  viewMode: GridViewMode;
  targetSize: number;
  showName: boolean;
  showExtension: boolean;
  showExtensionLabel?: boolean;
  showResolution?: boolean;
  fitThumbnails?: boolean;
  onTileClick?: (index: number, item: CanonicalEntityGridItem, event?: React.MouseEvent) => void;
  onTileDoubleClick?: (index: number, item: CanonicalEntityGridItem) => void;
  onEmptyClick?: () => void;
  onTileContextMenu?: (index: number, item: CanonicalEntityGridItem, position: { x: number; y: number }) => void;
  onEmptyContextMenu?: (position: { x: number; y: number }) => void;
  onLoadMore?: () => void;
  onFirstPaint?: () => void;
  onScrollTopChange?: (scrollTop: number) => void;
  interactive?: boolean;
  suppressTileReveal?: boolean;
  /** Restore scroll position on first paint (e.g., after back/forward navigation). */
  initialScrollTop?: number | null;
  selectedItemIds?: Set<number>;
  selectedFolderNodeIds?: Set<string>;
  onSelectionChange?: (itemIds: Set<number>) => void;
  onMarqueeSelectionChange?: (selection: { itemIds: Set<number>; folderNodeIds: Set<string> }) => void;
  collectHeaderMarqueeHits?: (rect: { left: number; top: number; width: number; height: number }) => Set<string>;
  /** DOM content rendered above the canvas inside the scroll container (e.g. SubfolderGrid). */
  headerContent?: React.ReactNode;
  /** Current scope for drag-and-drop context. */
  dragSourceScope?: ItemScope | null;
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
  showExtensionLabel = false,
  showResolution = false,
  fitThumbnails = false,
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
  suppressTileReveal = false,
  initialScrollTop = null,
  selectedItemIds = EMPTY_ITEM_ID_SET,
  selectedFolderNodeIds = EMPTY_FOLDER_NODE_SET,
  headerContent,
  dragSourceScope = null,
  onContainerRef,
  onLayoutChange,
  renamingIndex = null,
  onRenameCommit,
  onRenameCancel,
  onMarqueeSelectionChange,
  collectHeaderMarqueeHits,
}: CanvasGridProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const containerCallbackRef = useCallback((el: HTMLDivElement | null) => {
    (containerRef as React.MutableRefObject<HTMLDivElement | null>).current = el;
    onContainerRef?.(el);
  }, [onContainerRef]);
  const headerRef = useRef<HTMLDivElement>(null);
  const baseCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const viewportLayerRef = useRef<HTMLDivElement>(null);
  const pipelineRef = useRef<ThumbnailPipeline | null>(null);
  const revealTrackerRef = useRef(new ThumbnailRevealTracker());
  const suppressTileRevealRef = useRef(suppressTileReveal);
  const suppressRevealUntilRef = useRef(0);

  // Reusable per-frame buffers — avoids allocating new arrays/sets every draw call.
  const activeTilesRef = useRef<number[]>([]);
  const activeHashesRef = useRef(new Set<string>());
  const viewportHashesRef = useRef(new Set<string>());
  const planTilesRef = useRef<PlanTile[]>([]);
  const activationBuffersRef = useRef({
    activeTiles: activeTilesRef.current,
    activeHashes: activeHashesRef.current,
    viewportHashes: viewportHashesRef.current,
    planTiles: planTilesRef.current,
  });

  // Cache theme CSS values — avoids getComputedStyle() on every draw frame
  const cachedThemeRef = useRef({
    placeholderBg: 'rgba(255,255,255,0.04)',
    borderRadius: 4,
    textPrimary: 'rgba(255,255,255,0.92)',
    textTertiary: 'rgba(255,255,255,0.36)',
    glassBorder: 'rgba(255,255,255,0.14)',
    tileBoundary: 'rgba(255,255,255,0.12)',
  });
  // Refresh theme cache when container mounts or theme changes
  const refreshTheme = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const s = getComputedStyle(el);
    cachedThemeRef.current = {
      placeholderBg: s.getPropertyValue('--color-surface-2').trim() || 'rgba(255,255,255,0.04)',
      borderRadius: 4,
      textPrimary: s.getPropertyValue('--color-text-primary').trim() || 'rgba(255,255,255,0.92)',
      textTertiary: s.getPropertyValue('--color-text-tertiary').trim() || 'rgba(255,255,255,0.36)',
      glassBorder: s.getPropertyValue('--color-border-primary').trim() || 'rgba(255,255,255,0.14)',
      tileBoundary: s.getPropertyValue('--color-border-secondary').trim() || 'rgba(255,255,255,0.12)',
    };
  }, []);
  const scrollStateRef = useRef<CanvasScrollState>(createIdleCanvasScrollState());
  const lastScrollTopRef = useRef(0);
  const lastScrollTimeRef = useRef(0);
  const idleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const prevLayoutRef = useRef<ReturnType<GridLayoutRuntime['update']> | null>(null);
  const prevItemsRef = useRef(items);
  const hoveredTileRef = useRef<number | null>(null);
  const isScrollingRef = useRef(false);
  const marqueeRef = useRef<{ startX: number; startY: number; active: boolean; shiftKey: boolean; lastClientX: number; lastClientY: number }>({
    startX: 0, startY: 0, active: false, shiftKey: false, lastClientX: 0, lastClientY: 0,
  });
  const marqueeRectRef = useRef<{ left: number; top: number; width: number; height: number } | null>(null);
  const marqueeBaseSelectionRef = useRef<Set<number>>(new Set());
  const marqueeBaseFolderSelectionRef = useRef<Set<string>>(new Set());
  const dragJustEndedRef = useRef(false);
  const tileDragRef = useRef<{ tileIdx: number; startClientX: number; startClientY: number } | null>(null);
  const reorderDropRef = useRef<{ dropIndex: number; dropSide: 'left' | 'right' } | null>(null);
  // True when the overlay canvas currently has no painted content — lets
  // scroll frames skip the overlay redraw entirely (selection borders are
  // viewport-space, so a non-empty overlay must redraw every scroll frame).
  const overlayBlankRef = useRef(true);
  const autoScrollRef = useRef<number | null>(null);
  const autoScrollSpeedRef = useRef(0);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [hoverPreview, setHoverPreview] = useState<{ fileHash: string; mime: string } | null>(null);
  const [marqueeVisual, setMarqueeVisual] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  const [dragGhost, setDragGhost] = useState<{ x: number; y: number; count: number; thumbnailHashes: string[] } | null>(null);
  const firstPaintRef = useRef(false);
  const onLoadMoreRef = useRef(onLoadMore);
  const onScrollTopChangeRef = useRef(onScrollTopChange);
  onLoadMoreRef.current = onLoadMore;
  onScrollTopChangeRef.current = onScrollTopChange;

  // Width changes settle before layout; height changes size the sticky canvas
  // imperatively and never enter React state.
  const [layoutWidth, setLayoutWidth] = useState({ width: 0, scrollbarWidth: 0 });
  const [headerHeight, setHeaderHeight] = useState(0);
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const textHeight = (showName ? TEXT_NAME_ROW_H : 0) + (showResolution ? TEXT_NAME_ROW_H : 0);

  const layoutRuntimeRef = useRef(new GridLayoutRuntime());
  const layoutModel = useMemo(() => layoutRuntimeRef.current.update(items, {
    width: layoutWidth.width,
    targetSize,
    gap: GAP,
    viewMode,
    textHeight,
    scrollbarWidth: layoutWidth.scrollbarWidth,
  }), [items, layoutWidth, targetSize, viewMode, textHeight]);
  const renderItemsRef = useRef(layoutModel.items);
  renderItemsRef.current = layoutModel.items;

  const onLayoutChangeRef = useRef(onLayoutChange);
  onLayoutChangeRef.current = onLayoutChange;
  useEffect(() => { onLayoutChangeRef.current?.(layoutModel); }, [layoutModel]);
  const spatialIndexRef = useRef(layoutModel.spatialIndex);
  spatialIndexRef.current = layoutModel.spatialIndex;
  // Reusable query output buffer — avoids per-frame allocation.
  const candidateBufRef = useRef<number[]>([]);

  // ── Scroll anchor on resize / zoom / viewMode / display toggle ──
  // Anchor on the TOP EDGE of the topmost visible tile. The image top
  // is stable when text height changes (names/resolution toggle adds
  // space below images, not above). For zoom and view mode changes
  // the tile top is still the most intuitive anchor point.
  //
  // useLayoutEffect so we run before the browser paints / clamps scrollTop.
  // lastScrollTopRef is the pre-layout scroll position (kept in sync by
  // handleScroll and the initialScrollTop restore).
  const selectedItemIdsRef = useRef(selectedItemIds);
  selectedItemIdsRef.current = selectedItemIds;

  useLayoutEffect(() => {
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = layoutModel;
    const prevItems = prevItemsRef.current;
    prevItemsRef.current = items;

    // No previous layout (first mount / first resize) or same layout object.
    if (!prev || prev === layoutModel) return;
    if (prev.positions.length === 0 || layoutModel.positions.length === 0) return;

    const container = containerRef.current;
    if (!container) return;
    const vh = container.clientHeight;
    if (vh === 0) return;

    const next = resolveGridScrollAnchor({
      previousPositions: prev.positions,
      nextPositions: layoutModel.positions,
      previousItems: prevItems,
      nextItems: items,
      selectedItemIds: selectedItemIdsRef.current,
      scrollTop: lastScrollTopRef.current,
      viewportHeight: vh,
    });
    if (next == null) return;

    container.scrollTop = next;
    lastScrollTopRef.current = next;
  }, [layoutModel, items]);

  // ── Thumbnail pipeline lifecycle ──
  useEffect(() => {
    const pipeline = new ThumbnailPipeline(() => {
      markDirty('base');
    }, (fileHash) => {
      const now = performance.now();
      const suppress = suppressTileRevealRef.current || now < suppressRevealUntilRef.current;
      for (const item of renderItemsRef.current) {
        if (item.thumbnailHash === fileHash) revealTrackerRef.current.onBitmapAvailable(item.hash, now, suppress);
      }
    });
    pipelineRef.current = pipeline;
    return () => {
      pipeline.clear();
      revealTrackerRef.current.clear();
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
  // Suppress thumbnail fade animation during grid transitions.
  // Thumbnails loaded during fade-out/fade-in appear instantly.
  const prevSuppressRef = useRef(suppressTileReveal);
  useEffect(() => {
    suppressTileRevealRef.current = suppressTileReveal;
    // When transition ends (suppress goes false), keep suppressing for 500ms
    // to cover thumbnails still in flight from the transition period.
    if (prevSuppressRef.current && !suppressTileReveal) {
      suppressRevealUntilRef.current = performance.now() + 500;
    }
    prevSuppressRef.current = suppressTileReveal;
  }, [suppressTileReveal]);

  const drawBase = useCallback(() => {
    const container = containerRef.current;
    const canvas = baseCanvasRef.current;
    const pipeline = pipelineRef.current;
    if (!container || !canvas || !pipeline) return;

    const vp = snapshotViewport(container);
    const ctx = canvas.getContext('2d', { desynchronized: true });
    if (!ctx) return;

    ensureCanvasSize(canvas, vp.containerWidth, vp.viewportHeight, vp.dpr);

    // Offset by header height — tile positions start at Y=0 but the header pushes canvas content down
    const scrollTop = Math.max(0, vp.scrollTop - headerHeight);
    const now = performance.now();

    ctx.save();
    ctx.scale(vp.dpr, vp.dpr);
    ctx.clearRect(0, 0, vp.containerWidth, vp.viewportHeight);

    // The activation zone controls decode residency. The actual viewport
    // independently controls entity reveal identity.
    const ACTIVATION_MARGIN = 100;
    const zoneTop = scrollTop - ACTIVATION_MARGIN;
    const zoneBottom = scrollTop + vp.viewportHeight + ACTIVATION_MARGIN;

    // Reuse arrays/sets across frames to avoid per-frame GC pressure.
    const activeTiles = activeTilesRef.current;
    const activeHashes = activeHashesRef.current;
    const viewportHashes = viewportHashesRef.current;
    const planTiles = planTilesRef.current;

    // Y-binned index lookup — masonry positions are NOT Y-sorted (tiles
    // placed in shortest column), so candidates come from the spatial index
    // and get a precise re-test here.
    const candidates = candidateBufRef.current;
    candidates.length = 0;
    layoutModel.spatialIndex.queryYRange(zoneTop, zoneBottom, candidates);
    collectThumbnailActivation(
      candidates,
      layoutModel.positions,
      layoutModel.items,
      zoneTop,
      zoneBottom,
      scrollTop,
      scrollTop + vp.viewportHeight,
      activationBuffersRef.current,
    );

    // Send plan to worker — deduplicates internally, only posts when visible set changes.
    pipeline.updatePlan(planTiles, scrollTop + vp.viewportHeight / 2);
    const suppressReveal = suppressTileRevealRef.current || now < suppressRevealUntilRef.current;
    revealTrackerRef.current.updateViewport(
      viewportHashes,
      now,
      (itemId) => {
        const item = layoutModel.items.find((candidate) => candidate.hash === itemId);
        return item != null && pipeline.get(item.thumbnailHash)?.thumb != null;
      },
      suppressReveal,
    );

    const drawCtx: DrawContext = {
      scrollTop,
      viewportHeight: vp.viewportHeight,
      textHeight,
      borderRadius: 4,
    };

    const hasActiveRevealFromDraw = drawCanvasBaseLayer({
      ctx,
      positions: layoutModel.positions,
      items: layoutModel.items,
      atlasGet: (hash) => pipeline.get(hash),
      now,
      activeTiles,
      draw: drawCtx,
      theme: cachedThemeRef.current,
      viewMode,
      fitThumbnails,
      showTileName: showName,
      showResolution,
      showExtension,
      showExtensionLabel,
    });

    ctx.restore();

    // Evict main-thread bitmaps outside the draw zone.
    // The worker handles load cancellation via the plan diff.
    pipeline.evictOutsideActive(activeHashes);

    // Continue animation loop for active reveals
    if (hasActiveRevealFromDraw) {
      markDirty('base');
    }

    // First paint notification
    if (!firstPaintRef.current && activeTiles.length > 0) {
      firstPaintRef.current = true;
      onFirstPaint?.();
    }
  }, [layoutModel, viewMode, fitThumbnails, showName, showExtension, showExtensionLabel, showResolution, suppressTileReveal, textHeight, headerHeight]);

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
    const scrollTop = Math.max(0, vp.scrollTop - headerHeight);

    // Draw selection borders on all selected tiles
    if (selectedItemIds.size > 0) {
      ctx.strokeStyle = '#3297FF';
      ctx.lineWidth = 2;
      ctx.beginPath();
      const visible = activeTilesRef.current;
      for (let k = 0; k < visible.length; k++) {
        const i = visible[k];
        if (!selectedItemIds.has(layoutModel.items[i]?.itemId)) continue;
        const pos = layoutModel.positions[i];
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
      const hovPos = layoutModel.positions[hovIdx];
      if (hovItem && hovPos && !hovItem.display_mime_type.startsWith('video/')) {
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
      const rdPos = layoutModel.positions[rd.dropIndex];
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

    // Coarse blank flags — conservative: a selection that is offscreen still
    // counts as content so scrolling it into view redraws correctly.
    overlayBlankRef.current =
      selectedItemIds.size === 0 &&
      hoveredTileRef.current == null &&
      reorderDropRef.current == null;

    ctx.restore();
  }, [layoutModel, items, selectedItemIds, textHeight, headerHeight]);

  // ── Shared RAF scheduler ──
  const drawBaseRef = useRef(drawBase);
  drawBaseRef.current = drawBase;
  const drawOverlayRef = useRef(drawOverlay);
  drawOverlayRef.current = drawOverlay;
  const { markDirty } = useCanvasRedrawScheduler({
    drawBaseRef,
    drawOverlayRef,
  });

  // ── Resize observer ──
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const sizeViewport = () => {
      if (viewportLayerRef.current) viewportLayerRef.current.style.height = `${container.clientHeight}px`;
      markDirty('both');
    };
    const settleWidth = () => {
      const w = container.clientWidth;
      const sbw = container.offsetWidth - w;
      setLayoutWidth((current) => (
        current.width === w && current.scrollbarWidth === sbw
          ? current
          : { width: w, scrollbarWidth: sbw }
      ));
    };

    sizeViewport();
    settleWidth();
    const observer = new ResizeObserver(() => {
      sizeViewport();
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
      resizeTimerRef.current = setTimeout(() => {
        resizeTimerRef.current = null;
        settleWidth();
      }, GRID_RESIZE_SETTLE_MS);
    });
    observer.observe(container);

    // Re-measure when page zoom changes (Cmd+/Cmd-) — DPR changes with zoom
    const dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    const handleDprChange = sizeViewport;
    dprQuery.addEventListener('change', handleDprChange);

    return () => {
      observer.disconnect();
      dprQuery.removeEventListener('change', handleDprChange);
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
    };
  }, [markDirty]);

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
  useEffect(() => { markDirty('both'); }, [layoutModel, markDirty]);
  useEffect(() => { markDirty('base'); }, [showName, showExtension, showResolution, viewMode, fitThumbnails, suppressTileReveal, markDirty]);
  useEffect(() => { markDirty('overlay'); }, [selectedItemIds, markDirty]);
  useEffect(() => { markDirty('both'); }, [headerHeight, markDirty]);

  // ── Redraw on theme change (canvas reads CSS variables, not reactive to theme) ──
  // Deliberately NOT observing the style attribute: AppShell writes
  // --inspector-width to it on every mousemove during inspector resize
  // (covered by the ResizeObserver), and zoom redraws come from the
  // explicit zoomController subscription below.
  useEffect(() => {
    refreshTheme();
    const observer = new MutationObserver(() => {
      refreshTheme();
      markDirty('both');
    });
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme', 'data-mantine-color-scheme'],
    });
    const unsubZoom = zoomController.subscribe(() => markDirty('both'));
    return () => {
      observer.disconnect();
      unsubZoom();
    };
  }, [markDirty, refreshTheme]);

  // ── Global pointer tracking during drag ──
  // Uses refs to avoid effect re-runs on every ghost position update.
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const layoutModelRef = useRef(layoutModel);
  layoutModelRef.current = layoutModel;
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

      // If cursor exits the window during drag, initiate native OS file drag
      if (e.clientX <= 0 || e.clientY <= 0 || e.clientX >= window.innerWidth || e.clientY >= window.innerHeight) {
        const state = getDragState();
        reorderDropRef.current = null;
        setDragGhost(null);

        const fileHashes = state.itemIds.slice(0, 3).map((itemId) =>
          itemsRef.current.find((item) => item.item_id === itemId)?.display_file_hash ?? '',
        );
        const iconUrl = createNativeDragImageUrl(
          fileHashes,
          state.itemIds.length,
          (fileHash) => pipelineRef.current?.get(fileHash)?.thumb ?? null,
        );

        setInternalDragOrigin(true);
        startNativeDragFn(fileHashes, iconUrl);
        dragJustEndedRef.current = true;
        markDirtyRef.current('overlay');
        return;
      }

      moveDrag(e.clientX, e.clientY);
      setDragGhost((prev) => prev ? { ...prev, x: e.clientX, y: e.clientY } : null);
      const scope = getDragState().sourceScope;
      if (scope?.kind === 'folder') {
        const ctr = containerRef.current;
        if (ctr) {
          const { x: cx, y: cy } = toLayoutCoords(e.clientX, e.clientY, ctr, headerHeightRef.current);
          // Build skip set from dragged item indices
          const draggedItemIds = new Set(getDragState().itemIds);
          const skipIdx = new Set<number>();
          const model = layoutModelRef.current;
          for (const itemId of draggedItemIds) {
            const index = model.itemIdToIndex.get(itemId);
            if (index != null) skipIdx.add(index);
          }
          const tgt = computeReorderTarget(model.positions, cx, cy, textHeightRef.current, skipIdx);
          reorderDropRef.current = tgt ? { dropIndex: tgt.index, dropSide: tgt.side } : null;
          markDirtyRef.current('overlay');
        }
      }
    };
    const onUp = (e: MouseEvent) => {
      if (isDragActive()) {
        // If cursor is outside the window, the onMove handler already triggered native drag
        if (e.clientX <= 0 || e.clientY <= 0 || e.clientX >= window.innerWidth || e.clientY >= window.innerHeight) {
          return;
        }
        const rd = reorderDropRef.current;
        const existingTarget = getDragState().dropTarget;
        if (rd && !existingTarget) {
          const orderedItemIds = planFolderReorder(
            itemsRef.current.map((item) => item.item_id),
            new Set(getDragState().itemIds),
            rd.dropIndex,
            rd.dropSide,
          );
          if (orderedItemIds.length > 0) setDropTarget({ kind: 'reorder', orderedItemIds });
        }
        reorderDropRef.current = null;
        // Preserve selection: re-select the dragged hashes after drop
        const draggedItemIds = new Set(getDragState().itemIds);
        endDrag();
        dragJustEndedRef.current = true; // suppress the click that follows mouseup
        onSelectionChangeRef.current?.(draggedItemIds);
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
    if (!container) return;

    const now = performance.now();
    const scrollTop = container.scrollTop;
    if (!interactive) {
      lastScrollTopRef.current = scrollTop;
      onScrollTopChangeRef.current?.(scrollTop);
      markDirty('both');
      return;
    }
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
    if (hoverHideTimerRef.current) { clearTimeout(hoverHideTimerRef.current); hoverHideTimerRef.current = null; }
    // Always clear preview on scroll — the tile moved away from cursor
    setHoverPreview(null);

    onScrollTopChangeRef.current?.(scrollTop);
    // Skip the overlay redraw when it has nothing to paint (common case:
    // selection-free scrolling). The !overlayBlankRef term guarantees one
    // clearing redraw after content disappears (e.g. hover button above).
    const overlayNeedsRedraw =
      selectedItemIdsRef.current.size > 0 ||
      reorderDropRef.current != null ||
      !overlayBlankRef.current;
    markDirty(overlayNeedsRedraw ? 'both' : 'base');

    // Transition to idle after inactivity
    if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
    idleTimerRef.current = setTimeout(() => {
      scrollStateRef.current = createIdleCanvasScrollState();
      isScrollingRef.current = false;
      markDirty('base');
    }, CANVAS_SCROLL_IDLE_DELAY_MS);

    // Load more trigger
    const distanceFromBottom = container.scrollHeight - scrollTop - container.clientHeight;
    if (distanceFromBottom < container.clientHeight * 3) {
      onLoadMoreRef.current?.();
    }
  }, [interactive, markDirty]);

  // ── Click handler ──
  const isInHeader = useCallback((target: EventTarget) => {
    return headerRef.current?.contains(target as Node) ?? false;
  }, []);

  /** Check if the event target is on an interactive element inside the header (folder tile, button, etc.) */
  const isOnHeaderInteractive = useCallback((target: EventTarget) => {
    const el = target as HTMLElement;
    if (!isInHeader(target)) return false;
    return !!el.closest('[data-grid-header-interactive]') || !!el.closest('button');
  }, [isInHeader]);

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
    const idx = hitTestTile(layoutModel.positions, x, y, textHeight, 0, layoutModel.positions.length);

    if (idx != null && items[idx]) {
      onTileClick?.(idx, items[idx], e);
    } else {
      onEmptyClick?.();
    }
  }, [items, layoutModel.positions, onTileClick, onEmptyClick, textHeight, isOnHeaderInteractive, headerHeight]);

  // ── Double-click handler ──
  const handleDoubleClick = useCallback((e: React.MouseEvent) => {
    if (isOnHeaderInteractive(e.target)) return;
    if (!onTileDoubleClick) return;
    const container = containerRef.current;
    if (!container) return;
    const { x, y } = toLayoutCoords(e.clientX, e.clientY, container, headerHeight);
    const idx = hitTestTile(layoutModel.positions, x, y, textHeight, 0, layoutModel.positions.length);
    if (idx != null && items[idx]) {
      onTileDoubleClick(idx, items[idx]);
    }
  }, [items, layoutModel.positions, onTileDoubleClick, textHeight, isOnHeaderInteractive, headerHeight]);

  // ── Zoom button hit test (bottom-right corner of tile image area) ──
  const isZoomButtonHit = useCallback((clientX: number, clientY: number, tileIdx: number): boolean => {
    const container = containerRef.current;
    if (!container) return false;
    const { x: mx, y: my } = toLayoutCoords(clientX, clientY, container, headerHeight);
    const pos = layoutModel.positions[tileIdx];
    if (!pos) return false;
    const imgH = pos.h - textHeight;
    const bgW = ZOOM_BTN_SIZE + 4;
    const bgH = ZOOM_BTN_SIZE + 2;
    const zx = pos.x + pos.w - bgW;
    const zy = pos.y + imgH - bgH;
    return mx >= zx && mx < zx + bgW && my >= zy && my < zy + bgH;
  }, [layoutModel.positions, textHeight, headerHeight]);

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
    const idx = hitTestTile(layoutModel.positions, x, y, textHeight, 0, layoutModel.positions.length);

    if (idx !== hoveredTileRef.current) {
      hoveredTileRef.current = idx;
      markDirty('overlay');
    }

    // Hover preview: triggered when cursor is over the zoom button area
    if (idx != null && isZoomButtonHit(e.clientX, e.clientY, idx)) {
      const item = items[idx];
      const isPreviewable = item && !item.display_mime_type.startsWith('video/');

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
              prev?.fileHash === item.display_file_hash
                ? prev
                : { fileHash: item.display_file_hash, mime: item.display_mime_type },
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
  }, [layoutModel.positions, textHeight, items, isZoomButtonHit, hoverPreview, markDirty, headerHeight]);

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

    const idx = hitTestTile(layoutModel.positions, x, y, textHeight, 0, layoutModel.positions.length);
    if (idx != null && items[idx]) {
      onTileContextMenu?.(idx, items[idx], pos);
    } else {
      onEmptyContextMenu?.(pos);
    }
  }, [items, layoutModel.positions, onTileContextMenu, onEmptyContextMenu, textHeight, headerHeight]);

  // Marquee rect → selected item IDs (canvas tiles via spatial index + folder
  // DOM tiles). Reads refs so the auto-scroll RAF tick never goes stale.
  const collectMarqueeHits = useCallback((left: number, top: number, width: number, height: number) => {
    const itemIds = new Set(marqueeBaseSelectionRef.current);
    const folderNodeIds = new Set(marqueeBaseFolderSelectionRef.current);
    const positions = layoutModelRef.current.positions;
    const curItems = itemsRef.current;
    const th = textHeightRef.current;
    const candidates = candidateBufRef.current;
    candidates.length = 0;
    spatialIndexRef.current.queryYRange(top, top + height, candidates);
    for (let k = 0; k < candidates.length; k++) {
      const i = candidates[k];
      const pos = positions[i];
      if (!pos || !curItems[i]) continue;
      const imgH = pos.h - th;
      if (pos.x + pos.w > left && pos.x < left + width &&
          pos.y + imgH > top && pos.y < top + height) {
        itemIds.add(curItems[i].item_id);
      }
    }
    for (const id of collectHeaderMarqueeHits?.({ left, top, width, height }) ?? []) folderNodeIds.add(id);
    return { itemIds, folderNodeIds };
  }, [collectHeaderMarqueeHits]);

  // ── Marquee drag handlers ──
  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return; // left button only
    if (isOnHeaderInteractive(e.target)) return; // folder tiles handle their own clicks
    const container = containerRef.current;
    if (!container) return;

    const { x, y } = toLayoutCoords(e.clientX, e.clientY, container, headerHeight);

    // If clicking on a tile, set up potential tile drag (not marquee)
    // Also position the invisible drag helper over this tile for HTML5 native drag-out
    const idx = hitTestTile(layoutModel.positions, x, y, textHeight, 0, layoutModel.positions.length);
    if (idx != null) {
      tileDragRef.current = { tileIdx: idx, startClientX: e.clientX, startClientY: e.clientY };
      return;
    }
    tileDragRef.current = null;

    const mRect = container.getBoundingClientRect();
    const mZoomY = mRect.height / (container.offsetHeight || 1);
    marqueeRef.current = { startX: x, startY: y, active: true, shiftKey: e.shiftKey, lastClientX: x, lastClientY: (e.clientY - mRect.top) / mZoomY };
    marqueeBaseSelectionRef.current = e.shiftKey || e.metaKey || e.ctrlKey
      ? new Set(selectedItemIds)
      : new Set();
    marqueeBaseFolderSelectionRef.current = e.shiftKey || e.metaKey || e.ctrlKey
      ? new Set(selectedFolderNodeIds)
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
            onMarqueeSelectionChange?.(collectMarqueeHits(l, t, w, h));
          }
          markDirty('both');
        }
        autoScrollRef.current = requestAnimationFrame(tick);
      };
      autoScrollRef.current = requestAnimationFrame(tick);
    }
  }, [layoutModel.positions, textHeight, selectedItemIds, selectedFolderNodeIds, headerHeight, collectMarqueeHits, markDirty, onMarqueeSelectionChange]);

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    // Check for tile drag initiation (5px threshold)
    if (tileDragRef.current && !isDragActive()) {
      const dx = e.clientX - tileDragRef.current.startClientX;
      const dy = e.clientY - tileDragRef.current.startClientY;
      if (Math.abs(dx) > 5 || Math.abs(dy) > 5) {
        const tileIdx = tileDragRef.current.tileIdx;
        const item = items[tileIdx];
        if (item) {
          const itemId = item.item_id;
          const currentSelection = selectedItemIdsRef.current;
          const itemIds = currentSelection.has(itemId)
            ? [...currentSelection]
            : [itemId];
          const thumbHashes = itemIds.slice(0, 3).map((id) => {
            return items.find((candidate) => candidate.item_id === id)?.display_file_hash ?? '';
          });
          startDrag(itemIds, e.clientX, e.clientY, dragSourceScope);
          setDragGhost({ x: e.clientX, y: e.clientY, count: itemIds.length, thumbnailHashes: thumbHashes });
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
    onMarqueeSelectionChange?.(collectMarqueeHits(left, top, width, height));
    markDirty('overlay');
  }, [items, onMarqueeSelectionChange, markDirty, collectMarqueeHits, headerHeight]);

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
          <div className={styles.marquee} style={{
            left: marqueeVisual.left,
            top: marqueeVisual.top,
            width: marqueeVisual.width,
            height: marqueeVisual.height,
          }} />
        )}
        {renamingIndex != null && (() => {
          const pos = layoutModel.positions[renamingIndex];
          const item = items[renamingIndex];
          if (!pos || !item) return null;
          const imageH = pos.h - textHeight;
          return (
            <GlassInput
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
                textAlign: 'center',
                padding: '0 4px',
                borderRadius: 4,
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
          data-grid-layout
          style={{ height: `${layoutModel.totalHeight}px` }}
        >
          <div
            ref={viewportLayerRef}
            className={styles.viewportLayer}
          >
            <canvas
              ref={baseCanvasRef}
              className={styles.baseCanvas}
            />
            <canvas
              ref={overlayCanvasRef}
              className={styles.overlayCanvas}
            />
          </div>
        </div>
      </div>
      {hoverPreview && <HoverPreviewPortal fileHash={hoverPreview.fileHash} mime={hoverPreview.mime} />}
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
