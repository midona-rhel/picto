/**
 * Canvas grid — dual-canvas renderer with visibility-based thumbnail loading.
 *
 * Architecture:
 *   - Scroll container holds a sized div (totalHeight) for native scrollbar
 *   - Base canvas: thumbnails, placeholders, badges, text, borders
 *   - Overlay canvas: selection borders, hover ring
 *   - Layout positions computed from aspect ratios via layoutMath
 *   - Thumbnail pipeline loads ImageBitmaps for visible items
 *   - RAF-batched redraws on scroll, thumbnail load, or state change
 */

import { useEffect, useRef, useCallback, useMemo } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode } from '../layout/types';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { buildVisibilityPlan } from './visibilityPlan';
import { drawBaseLayer } from './drawBase';
import { drawOverlayLayer } from './drawOverlay';
import { hitTestTile } from './hitTesting';
import { ThumbnailPipeline } from './thumbnailPipeline';
import styles from './CanvasGrid.module.css';

const GAP = 4;
const TEXT_NAME_ROW_H = 20;
const PADDING_X = 4;

interface CanvasGridProps {
  items: CanonicalEntityGridItem[];
  viewMode: GridViewMode;
  targetSize: number;
  showName: boolean;
  showExtension: boolean;
  onTileClick?: (index: number, item: CanonicalEntityGridItem) => void;
  onLoadMore?: () => void;
}

export function CanvasGrid({
  items, viewMode, targetSize, showName, showExtension,
  onTileClick, onLoadMore,
}: CanvasGridProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const baseCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const hoverIndexRef = useRef<number | null>(null);
  const rafRef = useRef<number | null>(null);
  const dirtyRef = useRef<{ base: boolean; overlay: boolean }>({ base: true, overlay: true });

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;

  // Compute layout from aspect ratios
  const aspectRatios = useMemo(
    () => items.map((item) => {
      if (item.pixel_width && item.pixel_height) {
        return safeAspectRatio(item.pixel_width / item.pixel_height);
      }
      return 1.5; // Default for items without dimensions
    }),
    [items],
  );

  // Layout ref — recomputed when inputs change, but not via React state
  const layoutRef = useRef<ReturnType<typeof computeLayout>>({ positions: [], totalHeight: 0 });
  const containerWidthRef = useRef(0);

  // Thumbnail pipeline
  const pipelineRef = useRef<ThumbnailPipeline | null>(null);
  if (!pipelineRef.current) {
    pipelineRef.current = new ThumbnailPipeline(() => {
      dirtyRef.current.base = true;
      scheduleRedraw();
    });
  }

  // Recompute layout
  const recomputeLayout = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    const width = container.clientWidth;
    containerWidthRef.current = width;
    layoutRef.current = computeLayout(aspectRatios, width, targetSize, GAP, viewMode, textHeight, PADDING_X);

    // Size the canvas wrapper to match total height
    const wrap = container.querySelector(`.${styles.canvasWrap}`) as HTMLElement | null;
    if (wrap) wrap.style.height = `${layoutRef.current.totalHeight}px`;

    dirtyRef.current.base = true;
    dirtyRef.current.overlay = true;
    scheduleRedraw();
  }, [aspectRatios, targetSize, viewMode, textHeight]);

  // Redraw scheduling
  const scheduleRedraw = useCallback(() => {
    if (rafRef.current != null) return;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      draw();
    });
  }, []);

  // Main draw function
  const draw = useCallback(() => {
    const container = containerRef.current;
    const baseCanvas = baseCanvasRef.current;
    const overlayCanvas = overlayCanvasRef.current;
    if (!container || !baseCanvas || !overlayCanvas) return;

    const dpr = window.devicePixelRatio || 1;
    const width = container.clientWidth;
    const height = container.clientHeight;
    const scrollTop = container.scrollTop;

    // Resize canvases if needed
    const cw = Math.ceil(width * dpr);
    const ch = Math.ceil(height * dpr);
    if (baseCanvas.width !== cw || baseCanvas.height !== ch) {
      baseCanvas.width = cw;
      baseCanvas.height = ch;
      baseCanvas.style.width = `${width}px`;
      baseCanvas.style.height = `${height}px`;
      overlayCanvas.width = cw;
      overlayCanvas.height = ch;
      overlayCanvas.style.width = `${width}px`;
      overlayCanvas.style.height = `${height}px`;
      dirtyRef.current.base = true;
      dirtyRef.current.overlay = true;
    }

    const { positions } = layoutRef.current;
    const plan = buildVisibilityPlan(positions, scrollTop, height);

    // Request thumbnails for visible + prefetch items
    const pipeline = pipelineRef.current!;
    const visibleHashes: string[] = [];
    for (let i = plan.start; i < plan.end && i < items.length; i++) {
      visibleHashes.push(items[i].entity_hash);
    }
    for (const idx of plan.prefetchIndices) {
      if (idx < items.length) visibleHashes.push(items[idx].entity_hash);
    }
    pipeline.request(visibleHashes);

    // Evict thumbnails outside keep zone
    const keepSet = new Set(visibleHashes);
    pipeline.evict(keepSet);

    // Draw base layer
    if (dirtyRef.current.base) {
      const ctx = baseCanvas.getContext('2d')!;
      ctx.clearRect(0, 0, baseCanvas.width, baseCanvas.height);
      ctx.save();
      ctx.translate(0, -scrollTop * dpr);
      drawBaseLayer({
        ctx, items, positions,
        thumbnails: pipeline.getAll(),
        textHeight, visibleStart: plan.start, visibleEnd: plan.end,
        dpr, showName, showExtension,
      });
      ctx.restore();
      dirtyRef.current.base = false;
    }

    // Draw overlay layer
    if (dirtyRef.current.overlay) {
      const ctx = overlayCanvas.getContext('2d')!;
      ctx.clearRect(0, 0, overlayCanvas.width, overlayCanvas.height);
      ctx.save();
      ctx.translate(0, -scrollTop * dpr);
      drawOverlayLayer({
        ctx, positions, textHeight,
        visibleStart: plan.start, visibleEnd: plan.end,
        selectedIndices: new Set(), // TODO: selection state from PBI-593
        hoverIndex: hoverIndexRef.current,
        dpr,
      });
      ctx.restore();
      dirtyRef.current.overlay = false;
    }
  }, [items, textHeight, showName, showExtension]);

  // Scroll handler
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    function handleScroll() {
      dirtyRef.current.base = true;
      dirtyRef.current.overlay = true;
      scheduleRedraw();

      // Load more when near bottom
      if (onLoadMore && container) {
        const { scrollTop, scrollHeight, clientHeight } = container;
        if (scrollHeight - scrollTop - clientHeight < 400) {
          onLoadMore();
        }
      }
    }
    container.addEventListener('scroll', handleScroll, { passive: true });
    return () => container.removeEventListener('scroll', handleScroll);
  }, [scheduleRedraw, onLoadMore]);

  // Resize observer
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const ro = new ResizeObserver(() => recomputeLayout());
    ro.observe(container);
    return () => ro.disconnect();
  }, [recomputeLayout]);

  // Recompute layout when items/viewMode/targetSize change
  useEffect(() => {
    recomputeLayout();
  }, [recomputeLayout]);

  // Clear pipeline when items change (new scope)
  useEffect(() => {
    pipelineRef.current?.clear();
    dirtyRef.current.base = true;
    scheduleRedraw();
  }, [items]); // eslint-disable-line react-hooks/exhaustive-deps

  // Pointer interactions on overlay canvas
  useEffect(() => {
    const overlay = overlayCanvasRef.current;
    const container = containerRef.current;
    if (!overlay || !container) return;

    function getCanvasCoords(e: MouseEvent): { x: number; y: number } {
      const rect = container!.getBoundingClientRect();
      return { x: e.clientX - rect.left, y: e.clientY - rect.top + container!.scrollTop };
    }

    function handleMouseMove(e: MouseEvent) {
      const { x, y } = getCanvasCoords(e);
      const { positions } = layoutRef.current;
      const plan = buildVisibilityPlan(positions, container!.scrollTop, container!.clientHeight);
      const hit = hitTestTile(positions, x, y, textHeight, plan.start, plan.end);
      if (hit !== hoverIndexRef.current) {
        hoverIndexRef.current = hit;
        dirtyRef.current.overlay = true;
        scheduleRedraw();
      }
    }

    function handleMouseLeave() {
      if (hoverIndexRef.current !== null) {
        hoverIndexRef.current = null;
        dirtyRef.current.overlay = true;
        scheduleRedraw();
      }
    }

    function handleClick(e: MouseEvent) {
      const { x, y } = getCanvasCoords(e);
      const { positions } = layoutRef.current;
      const plan = buildVisibilityPlan(positions, container!.scrollTop, container!.clientHeight);
      const hit = hitTestTile(positions, x, y, textHeight, plan.start, plan.end);
      if (hit !== null && hit < items.length) {
        onTileClick?.(hit, items[hit]);
      }
    }

    overlay.addEventListener('mousemove', handleMouseMove);
    overlay.addEventListener('mouseleave', handleMouseLeave);
    overlay.addEventListener('click', handleClick);
    return () => {
      overlay.removeEventListener('mousemove', handleMouseMove);
      overlay.removeEventListener('mouseleave', handleMouseLeave);
      overlay.removeEventListener('click', handleClick);
    };
  }, [items, textHeight, onTileClick, scheduleRedraw]);

  // Cleanup
  useEffect(() => {
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
      pipelineRef.current?.clear();
    };
  }, []);

  return (
    <div className={styles.container} ref={containerRef}>
      <div className={styles.canvasWrap}>
        <canvas ref={baseCanvasRef} className={styles.baseCanvas} />
        <canvas ref={overlayCanvasRef} className={styles.overlayCanvas} />
      </div>
    </div>
  );
}
