import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode, LayoutResult } from '../layout/types';
import { HoverPreviewPortal } from './HoverPreviewPortal';
import { drawCanvasBaseLayer, type DrawContext } from './drawBase';

import { ThumbnailPipeline, type PlanTile } from './thumbnailPipeline';
import { ThumbnailRevealTracker } from './thumbnailRevealTracker';
import { zoomController } from '../../../controllers/zoomController';
import { estimateGridScrollHeight, GridLayoutRuntime } from './gridLayoutModel';
import { startDrag, moveDrag, endDrag, cancelDrag, setDropTarget, getDragState, isDragActive, startNativeDrag as startNativeDragFn, setInternalDragOrigin } from '../dragState';
import { hitTestTile, computeReorderTarget } from './hitTesting';
import { DragGhost } from '../DragGhost';
import { createNativeDragImageUrl } from '../dragGhostSpec';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import { resolveGridScrollAnchor } from './gridScrollAnchor';
import { planFolderReorder } from './folderReorder';
import { collectThumbnailActivation } from './thumbnailActivation';
import {
  type CanvasScrollState,
  createIdleCanvasScrollState,
  classifyCanvasScrollPhase,
  resolveCanvasScrollDirection,
  CANVAS_SCROLL_IDLE_DELAY_MS,
} from './scrollState';
import { useCanvasRedrawScheduler } from './useCanvasRedrawScheduler';
import { snapshotViewport, ensureCanvasSize } from './canvasViewportUtils';
import { GRID_GAP, GRID_REORDER_COLOR, GRID_SELECTION_COLOR, GRID_TILE_RADIUS } from '../gridAppearance';
import styles from './CanvasGrid.module.css';

const TEXT_NAME_ROW_H = 20;
const EMPTY_HASH_SET = new Set<string>();

function toLayoutCoords(clientX: number, clientY: number, container: HTMLDivElement, headerHeight: number) {
  const rect = container.getBoundingClientRect();
  const zoomX = rect.width / (container.offsetWidth || 1);
  const zoomY = rect.height / (container.offsetHeight || 1);
  return {
    x: (clientX - rect.left) / zoomX,
    y: (clientY - rect.top) / zoomY + container.scrollTop - headerHeight,
  };
}
const ZOOM_BTN_SIZE = 24;
const HOVER_PREVIEW_DELAY_MS = 200;
const HOVER_HIDE_DELAY_MS = 90;
const GRID_RESIZE_SETTLE_MS = 180;

function useLatest<T>(value: T) {
  const ref = useRef(value);
  ref.current = value;
  return ref;
}

function zoomButtonRect(position: { x: number; y: number; w: number }, imageHeight: number, y = position.y) {
  const width = ZOOM_BTN_SIZE + 4;
  const height = ZOOM_BTN_SIZE + 2;
  return { x: position.x + position.w - width, y: y + imageHeight - height, width, height };
}

function drawZoomButton(ctx: CanvasRenderingContext2D, x: number, y: number, width: number, height: number) {
  ctx.fillStyle = 'rgba(0,0,0,.4)';
  ctx.beginPath();
  ctx.roundRect(x, y, width, height, [10, 0, GRID_TILE_RADIUS, 0]);
  ctx.fill();
  const cx = x + width / 2;
  const cy = y + height / 2;
  ctx.strokeStyle = 'rgba(255,255,255,.7)';
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.arc(cx, cy, 5, 0, Math.PI * 2);
  ctx.moveTo(cx + 3.5, cy + 3.5);
  ctx.lineTo(cx + 6, cy + 6);
  ctx.stroke();
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(cx, cy - 2.5); ctx.lineTo(cx, cy + 2.5);
  ctx.moveTo(cx - 2.5, cy); ctx.lineTo(cx + 2.5, cy);
  ctx.stroke();
}

export interface CanvasGridProps {
  items: CanonicalEntityGridItem[];
  viewMode: GridViewMode;
  targetSize: number;
  showName: boolean;
  showExtension: boolean;
  showExtensionLabel?: boolean;
  showResolution?: boolean;
  fitThumbnails?: boolean;
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
  suppressTileReveal?: boolean;
  initialScrollTop?: number | null;
  selectedEntityHashes?: Set<string>;
  selectedFolderNodeIds?: Set<string>;
  onSelectionChange?: (hashes: Set<string>) => void;
  onMarqueeSelectionChange?: (selection: { entityHashes: Set<string>; folderNodeIds: Set<string> }) => void;
  collectHeaderMarqueeHits?: (rect: { left: number; top: number; width: number; height: number }) => Set<string>;
  headerContent?: React.ReactNode;
  dragSourceScope?: { kind: string; id?: number | null; key?: string | null } | null;
  onContainerRef?: (el: HTMLDivElement | null) => void;
  onLayoutChange?: (layout: LayoutResult) => void;
  renamingIndex?: number | null;
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
  suppressTileReveal = false,
  initialScrollTop = null,
  selectedEntityHashes = EMPTY_HASH_SET,
  selectedFolderNodeIds = EMPTY_HASH_SET,
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

  const activeTilesRef = useRef<number[]>([]);
  const visibleTilesRef = useRef<number[]>([]);
  const activeHashesRef = useRef(new Set<string>());
  const viewportHashesRef = useRef(new Set<string>());
  const planTilesRef = useRef<PlanTile[]>([]);
  const activationBuffersRef = useRef({
    activeTiles: activeTilesRef.current,
    visibleTiles: visibleTilesRef.current,
    activeHashes: activeHashesRef.current,
    viewportHashes: viewportHashesRef.current,
    planTiles: planTilesRef.current,
  });

  const cachedThemeRef = useRef({
    placeholderBg: 'rgba(255,255,255,0.04)',
    borderRadius: GRID_TILE_RADIUS,
    textPrimary: 'rgba(255,255,255,0.92)',
    textTertiary: 'rgba(255,255,255,0.36)',
    glassBorder: 'rgba(255,255,255,0.14)',
    tileBoundary: 'rgba(255,255,255,0.12)',
  });
  const refreshTheme = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const s = getComputedStyle(el);
    cachedThemeRef.current = {
      placeholderBg: s.getPropertyValue('--color-surface-2').trim() || 'rgba(255,255,255,0.04)',
      borderRadius: GRID_TILE_RADIUS,
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
  const marqueeBaseSelectionRef = useRef<Set<string>>(new Set());
  const marqueeBaseFolderSelectionRef = useRef<Set<string>>(new Set());
  const dragJustEndedRef = useRef(false);
  const tileDragRef = useRef<{ tileIdx: number; startClientX: number; startClientY: number } | null>(null);
  const reorderDropRef = useRef<{ dropIndex: number; dropSide: 'left' | 'right' } | null>(null);
  const overlayBlankRef = useRef(true);
  const autoScrollRef = useRef<number | null>(null);
  const autoScrollSpeedRef = useRef(0);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [hoverPreview, setHoverPreview] = useState<{ hash: string; mime: string } | null>(null);
  const [marqueeVisual, setMarqueeVisual] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  const [dragGhost, setDragGhost] = useState<{ x: number; y: number; count: number; thumbnailHashes: string[] } | null>(null);
  const firstPaintRef = useRef(false);
  const onLoadMoreRef = useLatest(onLoadMore);
  const onScrollTopChangeRef = useLatest(onScrollTopChange);

  const [layoutWidth, setLayoutWidth] = useState({ width: 0, scrollbarWidth: 0 });
  const [headerHeight, setHeaderHeight] = useState(0);
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const textHeight = (showName ? TEXT_NAME_ROW_H : 0) + (showResolution ? TEXT_NAME_ROW_H : 0);

  const layoutRuntimeRef = useRef(new GridLayoutRuntime());
  const layoutModel = useMemo(() => layoutRuntimeRef.current.update(items, {
    width: layoutWidth.width,
    targetSize,
    gap: GRID_GAP,
    viewMode,
    textHeight,
    scrollbarWidth: layoutWidth.scrollbarWidth,
  }), [items, layoutWidth, targetSize, viewMode, textHeight]);
  const estimatedScrollHeight = useMemo(
    () => estimateGridScrollHeight(layoutModel.totalHeight, items.length, totalCount),
    [items.length, layoutModel.totalHeight, totalCount],
  );

  const onLayoutChangeRef = useLatest(onLayoutChange);
  useEffect(() => { onLayoutChangeRef.current?.(layoutModel); }, [layoutModel]);
  const spatialIndexRef = useRef(layoutModel.spatialIndex);
  spatialIndexRef.current = layoutModel.spatialIndex;
  const candidateBufRef = useRef<number[]>([]);

  const selectedHashesRef = useLatest(selectedEntityHashes);

  useLayoutEffect(() => {
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = layoutModel;
    const prevItems = prevItemsRef.current;
    prevItemsRef.current = items;

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
      selectedHashes: selectedHashesRef.current,
      scrollTop: lastScrollTopRef.current,
      viewportHeight: vh,
    });
    if (next == null) return;

    container.scrollTop = next;
    lastScrollTopRef.current = next;
  }, [layoutModel, items]);

  useEffect(() => {
    const pipeline = new ThumbnailPipeline(() => {
      markDirty('base');
    }, (hash) => {
      const now = performance.now();
      revealTrackerRef.current.onBitmapAvailable(
        hash,
        now,
        suppressTileRevealRef.current || now < suppressRevealUntilRef.current,
      );
    });
    pipelineRef.current = pipeline;
    return () => {
      pipeline.clear();
      revealTrackerRef.current.clear();
      pipelineRef.current = null;
      if (idleTimerRef.current) { clearTimeout(idleTimerRef.current); idleTimerRef.current = null; }
    };
  }, []);

  const prevItemsLengthForScrollRef = useRef(0);
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const prevLen = prevItemsLengthForScrollRef.current;
    prevItemsLengthForScrollRef.current = items.length;

    if (initialScrollTop != null && initialScrollTop > 0) {
      firstPaintRef.current = false;
      container.scrollTop = initialScrollTop;
      lastScrollTopRef.current = initialScrollTop;
    } else if (prevLen === 0 || items.length === 0) {
      firstPaintRef.current = false;
      container.scrollTop = 0;
      lastScrollTopRef.current = 0;
    }
  }, [items]); // eslint-disable-line react-hooks/exhaustive-deps -- initialScrollTop read once per items change

  const prevSuppressRef = useRef(suppressTileReveal);
  useEffect(() => {
    suppressTileRevealRef.current = suppressTileReveal;
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

    const scrollTop = Math.max(0, vp.scrollTop - headerHeight);
    const now = performance.now();

    ctx.save();
    ctx.scale(vp.dpr, vp.dpr);
    ctx.clearRect(0, 0, vp.containerWidth, vp.viewportHeight);

    const ACTIVATION_MARGIN = 100;
    const zoneTop = scrollTop - ACTIVATION_MARGIN;
    const zoneBottom = scrollTop + vp.viewportHeight + ACTIVATION_MARGIN;

    const activeHashes = activeHashesRef.current;
    const viewportHashes = viewportHashesRef.current;
    const planTiles = planTilesRef.current;

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

    pipeline.updatePlan(planTiles, scrollTop + vp.viewportHeight / 2);
    const suppressReveal = suppressTileRevealRef.current || now < suppressRevealUntilRef.current;
    revealTrackerRef.current.updateViewport(
      viewportHashes,
      now,
      (hash) => pipeline.get(hash)?.thumb != null,
      suppressReveal,
    );

    const drawCtx: DrawContext = {
      scrollTop,
      textHeight,
      borderRadius: GRID_TILE_RADIUS,
    };

    const hasActiveRevealFromDraw = drawCanvasBaseLayer({
      ctx,
      positions: layoutModel.positions,
      items: layoutModel.items,
      atlasGet: (hash) => pipeline.get(hash),
      revealProgress: (hash) => revealTrackerRef.current.getProgress(hash, now),
      visibleTiles: visibleTilesRef.current,
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

    pipeline.evictOutsideActive(activeHashes);
    if (hasActiveRevealFromDraw) {
      markDirty('base');
    }
    if (!firstPaintRef.current && visibleTilesRef.current.length > 0) {
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

    if (selectedEntityHashes.size > 0) {
      ctx.strokeStyle = GRID_SELECTION_COLOR;
      ctx.lineWidth = 2;
      ctx.beginPath();
      const visible = visibleTilesRef.current;
      for (let k = 0; k < visible.length; k++) {
        const i = visible[k];
        if (!selectedEntityHashes.has(layoutModel.items[i]?.hash)) continue;
        const pos = layoutModel.positions[i];
        if (!pos) continue;
        const drawY = pos.y - scrollTop;
        const imgH = pos.h - textHeight;
        ctx.roundRect(pos.x - 1, drawY - 1, pos.w + 2, imgH + 2, GRID_TILE_RADIUS);
      }
      ctx.stroke();
    }

    const hovIdx = hoveredTileRef.current;
    if (hovIdx != null && !isScrollingRef.current) {
      const hovItem = items[hovIdx];
      const hovPos = layoutModel.positions[hovIdx];
      if (hovItem && hovPos && !hovItem.mime_type.startsWith('video/')) {
        const drawY = hovPos.y - scrollTop;
        if (drawY + hovPos.h >= 0 && drawY <= vp.viewportHeight) {
          const imgH = hovPos.h - textHeight;
          const button = zoomButtonRect(hovPos, imgH, drawY);
          drawZoomButton(ctx, button.x, button.y, button.width, button.height);
        }
      }
    }

    const rd = reorderDropRef.current;
    if (rd) {
      const rdPos = layoutModel.positions[rd.dropIndex];
      if (rdPos) {
        const rdDrawY = rdPos.y - scrollTop;
        const rdImgH = rdPos.h - textHeight;
        const indicatorX = rd.dropSide === 'left'
          ? rdPos.x - GRID_GAP / 2
          : rdPos.x + rdPos.w + GRID_GAP / 2;

        ctx.strokeStyle = GRID_REORDER_COLOR;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(indicatorX, rdDrawY);
        ctx.lineTo(indicatorX, rdDrawY + rdImgH);
        ctx.stroke();

        ctx.fillStyle = GRID_REORDER_COLOR;
        ctx.beginPath();
        ctx.moveTo(indicatorX - 5, rdDrawY);
        ctx.lineTo(indicatorX + 5, rdDrawY);
        ctx.lineTo(indicatorX, rdDrawY + 7);
        ctx.closePath();
        ctx.fill();
      }
    }

    overlayBlankRef.current =
      selectedEntityHashes.size === 0 &&
      hoveredTileRef.current == null &&
      reorderDropRef.current == null;

    ctx.restore();
  }, [layoutModel, items, selectedEntityHashes, textHeight, headerHeight]);

  const drawBaseRef = useRef(drawBase);
  drawBaseRef.current = drawBase;
  const drawOverlayRef = useRef(drawOverlay);
  drawOverlayRef.current = drawOverlay;
  const { markDirty } = useCanvasRedrawScheduler({
    drawBaseRef,
    drawOverlayRef,
  });

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

    const dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    const handleDprChange = sizeViewport;
    dprQuery.addEventListener('change', handleDprChange);

    return () => {
      observer.disconnect();
      dprQuery.removeEventListener('change', handleDprChange);
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
    };
  }, [markDirty]);

  useEffect(() => {
    const el = headerRef.current;
    if (!el) { setHeaderHeight(0); return; }
    const measure = () => setHeaderHeight(el.offsetHeight);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [headerContent]);

  useEffect(() => { markDirty('both'); }, [layoutModel, markDirty]);
  useEffect(() => { markDirty('base'); }, [showName, showExtension, showResolution, viewMode, fitThumbnails, suppressTileReveal, markDirty]);
  useEffect(() => { markDirty('overlay'); }, [selectedEntityHashes, markDirty]);
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

  const itemsRef = useLatest(items);
  const layoutModelRef = useLatest(layoutModel);
  const textHeightRef = useLatest(textHeight);
  const headerHeightRef = useLatest(headerHeight);
  const markDirtyRef = useLatest(markDirty);
  const onSelectionChangeRef = useLatest(onSelectionChange);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!isDragActive()) return;

      if (e.clientX <= 0 || e.clientY <= 0 || e.clientX >= window.innerWidth || e.clientY >= window.innerHeight) {
        const state = getDragState();
        reorderDropRef.current = null;
        setDragGhost(null);

        const iconUrl = createNativeDragImageUrl(
          state.hashes.slice(0, 3),
          state.hashes.length,
          (hash) => pipelineRef.current?.get(hash)?.thumb ?? null,
        );

        setInternalDragOrigin(true);
        startNativeDragFn(state.hashes, iconUrl);
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
          const draggedHashes = new Set(getDragState().hashes);
          const skipIdx = new Set<number>();
          const model = layoutModelRef.current;
          for (const hash of draggedHashes) {
            const index = model.hashToIndex.get(hash);
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
        if (e.clientX <= 0 || e.clientY <= 0 || e.clientX >= window.innerWidth || e.clientY >= window.innerHeight) {
          return;
        }
        const rd = reorderDropRef.current;
        const existingTarget = getDragState().dropTarget;
        if (rd && !existingTarget) {
          const moves = planFolderReorder(
            itemsRef.current.map((item) => item.entity_hash),
            new Set(getDragState().hashes),
            rd.dropIndex,
            rd.dropSide,
          );
          if (moves.length > 0) setDropTarget({ kind: 'reorder', moves });
        }
        reorderDropRef.current = null;
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

    if (hoveredTileRef.current != null) {
      hoveredTileRef.current = null;
    }
    if (hoverTimerRef.current) { clearTimeout(hoverTimerRef.current); hoverTimerRef.current = null; }
    if (hoverHideTimerRef.current) { clearTimeout(hoverHideTimerRef.current); hoverHideTimerRef.current = null; }
    setHoverPreview(null);

    onScrollTopChangeRef.current?.(scrollTop);
    // Skip the overlay redraw when it has nothing to paint (common case:
    // selection-free scrolling). The !overlayBlankRef term guarantees one
    // clearing redraw after content disappears (e.g. hover button above).
    const overlayNeedsRedraw =
      selectedHashesRef.current.size > 0 ||
      reorderDropRef.current != null ||
      !overlayBlankRef.current;
    markDirty(overlayNeedsRedraw ? 'both' : 'base');

    if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
    idleTimerRef.current = setTimeout(() => {
      scrollStateRef.current = createIdleCanvasScrollState();
      isScrollingRef.current = false;
      markDirty('base');
    }, CANVAS_SCROLL_IDLE_DELAY_MS);

    const distanceFromLoadedEnd = headerHeightRef.current + layoutModel.totalHeight
      - scrollTop - container.clientHeight;
    if (distanceFromLoadedEnd < container.clientHeight * 3) {
      onLoadMoreRef.current?.();
    }
  }, [interactive, layoutModel.totalHeight, markDirty]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !onLoadMore) return;
    const distanceFromLoadedEnd = headerHeight + layoutModel.totalHeight
      - container.scrollTop - container.clientHeight;
    if (distanceFromLoadedEnd < container.clientHeight * 3) onLoadMore();
  }, [headerHeight, layoutModel.totalHeight, onLoadMore]);

  const isInHeader = useCallback((target: EventTarget) => {
    return headerRef.current?.contains(target as Node) ?? false;
  }, []);

  const isOnHeaderInteractive = useCallback((target: EventTarget) => {
    const el = target as HTMLElement;
    if (!isInHeader(target)) return false;
    return !!el.closest('[data-grid-header-interactive]') || !!el.closest('button');
  }, [isInHeader]);

  const handleClick = useCallback((e: React.MouseEvent) => {
    if (dragJustEndedRef.current) {
      dragJustEndedRef.current = false;
      return;
    }
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

  const isZoomButtonHit = useCallback((clientX: number, clientY: number, tileIdx: number): boolean => {
    const container = containerRef.current;
    if (!container) return false;
    const { x: mx, y: my } = toLayoutCoords(clientX, clientY, container, headerHeight);
    const pos = layoutModel.positions[tileIdx];
    if (!pos) return false;
    const imgH = pos.h - textHeight;
    const button = zoomButtonRect(pos, imgH);
    return mx >= button.x && mx < button.x + button.width && my >= button.y && my < button.y + button.height;
  }, [layoutModel.positions, textHeight, headerHeight]);

  const clearHoverTimers = useCallback(() => {
    if (hoverTimerRef.current) { clearTimeout(hoverTimerRef.current); hoverTimerRef.current = null; }
    if (hoverHideTimerRef.current) { clearTimeout(hoverHideTimerRef.current); hoverHideTimerRef.current = null; }
  }, []);

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

    if (idx != null && isZoomButtonHit(e.clientX, e.clientY, idx)) {
      const item = items[idx];
      const isPreviewable = item && !item.mime_type.startsWith('video/');

      if (hoverHideTimerRef.current) {
        clearTimeout(hoverHideTimerRef.current);
        hoverHideTimerRef.current = null;
      }

      if (isPreviewable && !hoverTimerRef.current) {
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

  const collectMarqueeHits = useCallback((left: number, top: number, width: number, height: number) => {
    const entityHashes = new Set(marqueeBaseSelectionRef.current);
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
        entityHashes.add(curItems[i].entity_hash);
      }
    }
    for (const id of collectHeaderMarqueeHits?.({ left, top, width, height }) ?? []) folderNodeIds.add(id);
    return { entityHashes, folderNodeIds };
  }, [collectHeaderMarqueeHits]);

  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return; // left button only
    if (isOnHeaderInteractive(e.target)) return; // folder tiles handle their own clicks
    const container = containerRef.current;
    if (!container) return;

    const { x, y } = toLayoutCoords(e.clientX, e.clientY, container, headerHeight);

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
      ? new Set(selectedEntityHashes)
      : new Set();
    marqueeBaseFolderSelectionRef.current = e.shiftKey || e.metaKey || e.ctrlKey
      ? new Set(selectedFolderNodeIds)
      : new Set();
    marqueeRectRef.current = null;
    autoScrollSpeedRef.current = 0;
    container.setPointerCapture(e.pointerId);

    if (autoScrollRef.current == null) {
      const tick = () => {
        if (!marqueeRef.current.active) { autoScrollRef.current = null; return; }
        const c = containerRef.current;
        if (c && autoScrollSpeedRef.current !== 0) {
          c.scrollTop += autoScrollSpeedRef.current;
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
  }, [layoutModel.positions, textHeight, selectedEntityHashes, selectedFolderNodeIds, headerHeight, collectMarqueeHits, markDirty, onMarqueeSelectionChange]);

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
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
            return it?.entity_hash ?? h;
          });
          startDrag(hashes, e.clientX, e.clientY, dragSourceScope);
          setDragGhost({ x: e.clientX, y: e.clientY, count: hashes.length, thumbnailHashes: thumbHashes });
          reorderDropRef.current = null;
          tileDragRef.current = null;
        }
      }
      return;
    }
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
    setMarqueeVisual({ left, top: top + headerHeight, width, height });
    onMarqueeSelectionChange?.(collectMarqueeHits(left, top, width, height));
    markDirty('overlay');
  }, [items, onMarqueeSelectionChange, markDirty, collectMarqueeHits, headerHeight]);

  const handlePointerUp = useCallback((e: React.PointerEvent) => {
    tileDragRef.current = null;
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
                borderRadius: GRID_TILE_RADIUS,
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
          style={{ height: `${estimatedScrollHeight}px` }}
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
