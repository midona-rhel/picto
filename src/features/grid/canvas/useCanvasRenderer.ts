import { useCallback, useEffect, useRef, type MutableRefObject, type RefObject } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode } from '../layout/types';
import { zoomController } from '../../../controllers/zoomController';
import { GRID_REORDER_COLOR, GRID_SELECTION_COLOR, GRID_TILE_RADIUS } from '../gridAppearance';
import { drawCanvasBaseLayer, type DrawContext } from './drawBase';
import type { GridLayoutModel } from './gridLayoutModel';
import { collectThumbnailActivation } from './thumbnailActivation';
import { ThumbnailPipeline, type PlanTile } from './thumbnailPipeline';
import { ThumbnailRevealTracker } from './thumbnailRevealTracker';
import { ensureCanvasSize, snapshotViewport } from './canvasViewportUtils';
import { useCanvasRedrawScheduler } from './useCanvasRedrawScheduler';

export type DirtyLanes = 'base' | 'overlay' | 'both';
export interface ReorderDrop { dropIndex: number; dropSide: 'left' | 'right' }

export function zoomButtonRect(position: { x: number; y: number; w: number }, imageHeight: number, y = position.y) {
  const width = 28;
  const height = 26;
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

interface CanvasRendererOptions {
  containerRef: RefObject<HTMLDivElement>;
  baseCanvasRef: RefObject<HTMLCanvasElement>;
  overlayCanvasRef: RefObject<HTMLCanvasElement>;
  layout: GridLayoutModel;
  sourceItems: CanonicalEntityGridItem[];
  viewMode: GridViewMode;
  textHeight: number;
  headerHeight: number;
  showName: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  showResolution: boolean;
  fitThumbnails: boolean;
  suppressTileReveal: boolean;
  selectedHashes: Set<string>;
  hoveredTileRef: MutableRefObject<number | null>;
  isScrollingRef: MutableRefObject<boolean>;
  reorderDropRef: MutableRefObject<ReorderDrop | null>;
  overlayBlankRef: MutableRefObject<boolean>;
  firstPaintRef: MutableRefObject<boolean>;
  onFirstPaint?: () => void;
}

/** Owns bitmap residency, reveal identity, theme state, and both canvas lanes. */
export function useCanvasRenderer(options: CanvasRendererOptions) {
  const {
    containerRef, baseCanvasRef, overlayCanvasRef, layout, sourceItems, viewMode,
    textHeight, headerHeight, showName, showExtension, showExtensionLabel,
    showResolution, fitThumbnails, suppressTileReveal, selectedHashes,
    hoveredTileRef, isScrollingRef, reorderDropRef, overlayBlankRef,
    firstPaintRef, onFirstPaint,
  } = options;
  const pipelineRef = useRef<ThumbnailPipeline | null>(null);
  const revealTrackerRef = useRef(new ThumbnailRevealTracker());
  const suppressRef = useRef(suppressTileReveal);
  const suppressUntilRef = useRef(0);
  const visibleTilesRef = useRef<number[]>([]);
  const candidateBuffer = useRef<number[]>([]);
  const activation = useRef({
    activeTiles: [] as number[],
    visibleTiles: visibleTilesRef.current,
    activeHashes: new Set<string>(),
    viewportHashes: new Set<string>(),
    planTiles: [] as PlanTile[],
  });
  const theme = useRef({
    placeholderBg: 'rgba(255,255,255,0.04)',
    borderRadius: GRID_TILE_RADIUS,
    textPrimary: 'rgba(255,255,255,0.92)',
    textTertiary: 'rgba(255,255,255,0.36)',
    glassBorder: 'rgba(255,255,255,0.14)',
    tileBoundary: 'rgba(255,255,255,0.12)',
  });
  const refreshTheme = useCallback(() => {
    const element = containerRef.current;
    if (!element) return;
    const style = getComputedStyle(element);
    theme.current = {
      placeholderBg: style.getPropertyValue('--color-surface-2').trim() || 'rgba(255,255,255,0.04)',
      borderRadius: GRID_TILE_RADIUS,
      textPrimary: style.getPropertyValue('--color-text-primary').trim() || 'rgba(255,255,255,0.92)',
      textTertiary: style.getPropertyValue('--color-text-tertiary').trim() || 'rgba(255,255,255,0.36)',
      glassBorder: style.getPropertyValue('--color-border-primary').trim() || 'rgba(255,255,255,0.14)',
      tileBoundary: style.getPropertyValue('--color-border-secondary').trim() || 'rgba(255,255,255,0.12)',
    };
  }, [containerRef]);

  const drawBase = useCallback(() => {
    const container = containerRef.current;
    const canvas = baseCanvasRef.current;
    const pipeline = pipelineRef.current;
    if (!container || !canvas || !pipeline) return;
    const viewport = snapshotViewport(container);
    const context = canvas.getContext('2d', { desynchronized: true });
    if (!context) return;
    ensureCanvasSize(canvas, viewport.containerWidth, viewport.viewportHeight, viewport.dpr);
    const scrollTop = Math.max(0, viewport.scrollTop - headerHeight);
    const now = performance.now();
    context.save();
    context.scale(viewport.dpr, viewport.dpr);
    context.clearRect(0, 0, viewport.containerWidth, viewport.viewportHeight);

    const activeTop = scrollTop - 100;
    const activeBottom = scrollTop + viewport.viewportHeight + 100;
    const candidates = candidateBuffer.current;
    candidates.length = 0;
    layout.spatialIndex.queryYRange(activeTop, activeBottom, candidates);
    collectThumbnailActivation(
      candidates, layout.positions, layout.items, activeTop, activeBottom,
      scrollTop, scrollTop + viewport.viewportHeight, activation.current,
    );
    pipeline.updatePlan(activation.current.planTiles, scrollTop + viewport.viewportHeight / 2);
    const suppress = suppressRef.current || now < suppressUntilRef.current;
    revealTrackerRef.current.updateViewport(
      activation.current.viewportHashes,
      now,
      (hash) => pipeline.get(hash)?.thumb != null,
      suppress,
    );
    const draw: DrawContext = { scrollTop, textHeight, borderRadius: GRID_TILE_RADIUS };
    const revealing = drawCanvasBaseLayer({
      ctx: context,
      positions: layout.positions,
      items: layout.items,
      atlasGet: (hash) => pipeline.get(hash),
      revealProgress: (hash) => revealTrackerRef.current.getProgress(hash, now),
      visibleTiles: visibleTilesRef.current,
      draw,
      theme: theme.current,
      viewMode,
      fitThumbnails,
      showTileName: showName,
      showResolution,
      showExtension,
      showExtensionLabel,
    });
    context.restore();
    pipeline.evictOutsideActive(activation.current.activeHashes);
    if (revealing) markDirtyRef.current('base');
    if (!firstPaintRef.current && visibleTilesRef.current.length > 0) {
      firstPaintRef.current = true;
      onFirstPaint?.();
    }
  }, [baseCanvasRef, containerRef, firstPaintRef, fitThumbnails, headerHeight, layout, onFirstPaint, showExtension, showExtensionLabel, showName, showResolution, textHeight, viewMode]);

  const drawOverlay = useCallback(() => {
    const container = containerRef.current;
    const canvas = overlayCanvasRef.current;
    if (!container || !canvas) return;
    const viewport = snapshotViewport(container);
    const context = canvas.getContext('2d', { desynchronized: true });
    if (!context) return;
    ensureCanvasSize(canvas, viewport.containerWidth, viewport.viewportHeight, viewport.dpr);
    context.clearRect(0, 0, canvas.width, canvas.height);
    context.save();
    context.scale(viewport.dpr, viewport.dpr);
    const scrollTop = Math.max(0, viewport.scrollTop - headerHeight);

    if (selectedHashes.size > 0) {
      context.strokeStyle = GRID_SELECTION_COLOR;
      context.lineWidth = 2;
      context.beginPath();
      for (const index of visibleTilesRef.current) {
        if (!selectedHashes.has(layout.items[index]?.hash)) continue;
        const position = layout.positions[index];
        if (!position) continue;
        context.roundRect(position.x - 1, position.y - scrollTop - 1, position.w + 2, position.h - textHeight + 2, GRID_TILE_RADIUS);
      }
      context.stroke();
    }

    const hovered = hoveredTileRef.current;
    if (hovered != null && !isScrollingRef.current) {
      const item = sourceItems[hovered];
      const position = layout.positions[hovered];
      if (item && position && !item.mime_type.startsWith('video/')) {
        const y = position.y - scrollTop;
        if (y + position.h >= 0 && y <= viewport.viewportHeight) {
          const button = zoomButtonRect(position, position.h - textHeight, y);
          drawZoomButton(context, button.x, button.y, button.width, button.height);
        }
      }
    }

    const reorder = reorderDropRef.current;
    const position = reorder ? layout.positions[reorder.dropIndex] : null;
    if (reorder && position) {
      const y = position.y - scrollTop;
      const x = reorder.dropSide === 'left'
        ? position.x - 8
        : position.x + position.w + 8;
      context.strokeStyle = GRID_REORDER_COLOR;
      context.lineWidth = 2;
      context.beginPath(); context.moveTo(x, y); context.lineTo(x, y + position.h - textHeight); context.stroke();
      context.fillStyle = GRID_REORDER_COLOR;
      context.beginPath(); context.moveTo(x - 5, y); context.lineTo(x + 5, y); context.lineTo(x, y + 7); context.closePath(); context.fill();
    }
    overlayBlankRef.current = selectedHashes.size === 0 && hovered == null && reorder == null;
    context.restore();
  }, [containerRef, headerHeight, hoveredTileRef, isScrollingRef, layout, overlayBlankRef, overlayCanvasRef, reorderDropRef, selectedHashes, sourceItems, textHeight]);

  const drawBaseRef = useRef(drawBase);
  drawBaseRef.current = drawBase;
  const drawOverlayRef = useRef(drawOverlay);
  drawOverlayRef.current = drawOverlay;
  const { markDirty } = useCanvasRedrawScheduler({ drawBaseRef, drawOverlayRef });
  const markDirtyRef = useRef(markDirty);
  markDirtyRef.current = markDirty;

  useEffect(() => {
    const pipeline = new ThumbnailPipeline(
      () => markDirtyRef.current('base'),
      (hash) => {
        const now = performance.now();
        revealTrackerRef.current.onBitmapAvailable(
          hash, now, suppressRef.current || now < suppressUntilRef.current,
        );
      },
    );
    pipelineRef.current = pipeline;
    return () => {
      pipeline.clear();
      revealTrackerRef.current.clear();
      pipelineRef.current = null;
    };
  }, []);

  const previousSuppress = useRef(suppressTileReveal);
  useEffect(() => {
    suppressRef.current = suppressTileReveal;
    if (previousSuppress.current && !suppressTileReveal) suppressUntilRef.current = performance.now() + 500;
    previousSuppress.current = suppressTileReveal;
  }, [suppressTileReveal]);
  useEffect(() => { markDirty('both'); }, [headerHeight, layout, markDirty]);
  useEffect(() => { markDirty('base'); }, [fitThumbnails, markDirty, showExtension, showExtensionLabel, showName, showResolution, suppressTileReveal, viewMode]);
  useEffect(() => { markDirty('overlay'); }, [markDirty, selectedHashes]);
  useEffect(() => {
    refreshTheme();
    const observer = new MutationObserver(() => { refreshTheme(); markDirty('both'); });
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-mantine-color-scheme'] });
    const unsubscribeZoom = zoomController.subscribe(() => markDirty('both'));
    return () => { observer.disconnect(); unsubscribeZoom(); };
  }, [markDirty, refreshTheme]);

  return { markDirty, pipelineRef };
}
