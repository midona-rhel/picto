/**
 * Canvas grid — single-canvas renderer with visibility-based thumbnail loading.
 */

import { useEffect, useRef, useCallback, useMemo } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode } from '../layout/types';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { buildVisibilityPlan } from './visibilityPlan';
import { drawBaseLayer } from './drawBase';
import { hitTestTile } from './hitTesting';
import { ThumbnailPipeline } from './thumbnailPipeline';
import styles from './CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const PADDING_X = 10;
const REVEAL_DURATION_MS = 250;
const MAX_CONCURRENT_REVEALS = 54;

interface RevealState {
  startAt: number;
}

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
  const wrapRef = useRef<HTMLDivElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const baseCanvasRef = useRef<HTMLCanvasElement>(null);
  const baseContextRef = useRef<CanvasRenderingContext2D | null>(null);
  const rafRef = useRef<number | null>(null);
  const dirtyRef = useRef(true);
  const drawRef = useRef<() => void>(() => {});
  const firstPaintNotifiedRef = useRef(false);
  const lastVisibleSetRef = useRef<Set<string>>(new Set());
  const revealStatesRef = useRef<Map<string, RevealState>>(new Map());
  const revealSlotsRef = useRef<number[]>([]);
  const scrollDirectionRef = useRef<-1 | 0 | 1>(0);
  const lastScrollTopRef = useRef(0);
  const backgroundColorRef = useRef<string>('rgb(24, 25, 27)');

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;

  const scheduleRedraw = useCallback(() => {
    if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      drawRef.current();
    });
  }, []);

  const pipelineRef = useRef<ThumbnailPipeline | null>(null);
  useEffect(() => {
    if (!pipelineRef.current) {
      pipelineRef.current = new ThumbnailPipeline(() => {
        dirtyRef.current = true;
        scheduleRedraw();
      });
    }
    return () => {
      pipelineRef.current?.clear();
      pipelineRef.current = null;
    };
  }, [scheduleRedraw]);

  const aspectRatios = useMemo(
    () => items.map((item) => {
      if (item.pixel_width && item.pixel_height) {
        return safeAspectRatio(item.pixel_width / item.pixel_height);
      }
      return 1.5;
    }),
    [items],
  );

  const layoutRef = useRef<ReturnType<typeof computeLayout>>({ positions: [], totalHeight: 0 });
  const targetSizeRef = useRef(targetSize);
  targetSizeRef.current = targetSize;

  const recomputeLayout = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;

    const width = container.clientWidth;
    layoutRef.current = computeLayout(aspectRatios, width, targetSizeRef.current, GAP, viewMode, textHeight, PADDING_X);

    if (wrapRef.current) wrapRef.current.style.height = `${layoutRef.current.totalHeight}px`;
    if (viewportRef.current) viewportRef.current.style.height = `${container.clientHeight}px`;

    dirtyRef.current = true;
    scheduleRedraw();
  }, [aspectRatios, viewMode, textHeight, scheduleRedraw]);

  const nextRevealSlot = useCallback((now: number): number => {
    const activeSlots = revealSlotsRef.current.filter((t) => now - t < REVEAL_DURATION_MS);
    if (activeSlots.length < MAX_CONCURRENT_REVEALS) {
      activeSlots.push(now);
      revealSlotsRef.current = activeSlots;
      return now;
    }

    const oldest = activeSlots[0];
    const staggered = oldest + REVEAL_DURATION_MS;
    activeSlots.shift();
    activeSlots.push(staggered);
    revealSlotsRef.current = activeSlots;
    return staggered;
  }, []);

  const draw = useCallback(() => {
    const container = containerRef.current;
    const baseCanvas = baseCanvasRef.current;
    const pipeline = pipelineRef.current;
    if (!container || !baseCanvas || !pipeline) return;

    const dpr = window.devicePixelRatio || 1;
    const width = container.clientWidth;
    const height = container.clientHeight;
    const scrollTop = interactive ? container.scrollTop : frozenScrollTop;

    const pixelWidth = Math.ceil(width * dpr);
    const pixelHeight = Math.ceil(height * dpr);
    if (baseCanvas.width !== pixelWidth || baseCanvas.height !== pixelHeight) {
      baseCanvas.width = pixelWidth;
      baseCanvas.height = pixelHeight;
      baseCanvas.style.width = `${width}px`;
      baseCanvas.style.height = `${height}px`;
      baseContextRef.current = baseCanvas.getContext('2d', {
        alpha: false,
        desynchronized: true,
      });
      dirtyRef.current = true;
    }

    const ctx = baseContextRef.current;
    if (!ctx) return;

    backgroundColorRef.current = getComputedStyle(container).backgroundColor || backgroundColorRef.current;

    const { positions } = layoutRef.current;
    const plan = buildVisibilityPlan(positions, scrollTop, height, scrollDirectionRef.current);

    const visibleHashes: string[] = [];
    const aheadHashes: string[] = [];
    const behindHashes: string[] = [];

    const seen = new Set<string>();
    const collect = (indices: number[], out: string[]) => {
      for (const index of indices) {
        if (index < 0 || index >= items.length) continue;
        const hash = items[index].entity_hash;
        if (!hash || seen.has(hash)) continue;
        seen.add(hash);
        out.push(hash);
      }
    };

    for (let i = plan.start; i < plan.end && i < items.length; i++) {
      collect([i], visibleHashes);
    }
    collect(plan.aheadPrefetchIndices, aheadHashes);
    collect(plan.behindPrefetchIndices, behindHashes);

    pipeline.request({
      visible: visibleHashes,
      ahead: aheadHashes,
      behind: behindHashes,
    });
    pipeline.evict(new Set([...visibleHashes, ...aheadHashes, ...behindHashes]));

    const revealProgressByHash = new Map<string, number>();
    let needsNextAnimationFrame = false;
    if (suppressTileReveal) {
      for (const hash of visibleHashes) {
        revealProgressByHash.set(hash, 1);
      }
      // Track visible set so tiles don't re-fade when suppress ends
      lastVisibleSetRef.current = new Set(visibleHashes);
    } else {
      const now = performance.now();
      const visibleSet = new Set(visibleHashes);
      for (const hash of visibleHashes) {
        if (!lastVisibleSetRef.current.has(hash)) {
          revealStatesRef.current.set(hash, { startAt: nextRevealSlot(now) });
        }
      }
      for (const hash of revealStatesRef.current.keys()) {
        if (!visibleSet.has(hash)) revealStatesRef.current.delete(hash);
      }
      lastVisibleSetRef.current = visibleSet;

      for (const hash of visibleHashes) {
        const state = revealStatesRef.current.get(hash);
        if (!state) {
          revealProgressByHash.set(hash, 1);
          continue;
        }
        const progress = Math.max(0, Math.min(1, (now - state.startAt) / REVEAL_DURATION_MS));
        revealProgressByHash.set(hash, progress);
        if (progress < 1 && pipeline.get(hash)) {
          needsNextAnimationFrame = true;
        }
      }
    }

    if (dirtyRef.current || needsNextAnimationFrame) {
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = backgroundColorRef.current;
      ctx.fillRect(0, 0, baseCanvas.width, baseCanvas.height);
      ctx.restore();
      ctx.save();
      ctx.translate(0, -scrollTop * dpr);
      drawBaseLayer({
        ctx,
        items,
        positions,
        thumbnails: pipeline.getAll(),
        revealProgressByHash,
        textHeight,
        visibleStart: plan.start,
        visibleEnd: plan.end,
        dpr,
        showName,
        showExtension,
      });
      ctx.restore();
      dirtyRef.current = false;

      if (!firstPaintNotifiedRef.current && items.length > 0 && plan.end > plan.start) {
        firstPaintNotifiedRef.current = true;
        onFirstPaint?.();
      }
    }

    if (needsNextAnimationFrame) {
      dirtyRef.current = true;
      scheduleRedraw();
    }
  }, [
    items,
    frozenScrollTop,
    interactive,
    onFirstPaint,
    scheduleRedraw,
    showExtension,
    showName,
    suppressTileReveal,
    textHeight,
    nextRevealSlot,
  ]);

  drawRef.current = draw;

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !interactive) return;

    const handleScroll = () => {
      const scrollTop = container.scrollTop;
      const previous = lastScrollTopRef.current;
      scrollDirectionRef.current = scrollTop > previous ? 1 : scrollTop < previous ? -1 : 0;
      lastScrollTopRef.current = scrollTop;
      onScrollTopChange?.(scrollTop);
      dirtyRef.current = true;
      scheduleRedraw();

      if (onLoadMore) {
        const { scrollHeight, clientHeight } = container;
        if (scrollHeight - scrollTop - clientHeight < 400) {
          onLoadMore();
        }
      }
    };

    container.addEventListener('scroll', handleScroll, { passive: true });
    return () => container.removeEventListener('scroll', handleScroll);
  }, [interactive, onLoadMore, onScrollTopChange, scheduleRedraw]);

  // Debounced resize — freeze layout during active window drag
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let timer: number | null = null;
    const DEBOUNCE = 150;

    const ro = new ResizeObserver(() => {
      if (timer != null) clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = null;
        recomputeLayout();
      }, DEBOUNCE);
    });

    ro.observe(container);
    return () => {
      ro.disconnect();
      if (timer != null) clearTimeout(timer);
    };
  }, [recomputeLayout]);

  // Immediate layout on items/viewMode change
  useEffect(() => {
    recomputeLayout();
  }, [recomputeLayout]);

  // Debounced layout on zoom slider drag
  useEffect(() => {
    const timer = window.setTimeout(() => recomputeLayout(), 80);
    return () => clearTimeout(timer);
  }, [targetSize]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    firstPaintNotifiedRef.current = false;
    lastVisibleSetRef.current = new Set();
    revealStatesRef.current.clear();
    revealSlotsRef.current = [];
    dirtyRef.current = true;
    scheduleRedraw();
  }, [items, viewMode, targetSize, showName, showExtension, scheduleRedraw]);

  useEffect(() => {
    if (!interactive) {
      lastScrollTopRef.current = frozenScrollTop;
      scrollDirectionRef.current = 0;
      dirtyRef.current = true;
      scheduleRedraw();
    }
  }, [frozenScrollTop, interactive, scheduleRedraw]);

  useEffect(() => {
    const baseCanvas = baseCanvasRef.current;
    const container = containerRef.current;
    if (!baseCanvas || !container) return;

    const getCanvasCoords = (e: MouseEvent): { x: number; y: number } => {
      const rect = container.getBoundingClientRect();
      const scrollTop = interactive ? container.scrollTop : frozenScrollTop;
      return {
        x: e.clientX - rect.left,
        y: e.clientY - rect.top + scrollTop,
      };
    };

    const handleClick = (e: MouseEvent) => {
      const { x, y } = getCanvasCoords(e);
      const { positions } = layoutRef.current;
      const scrollTop = interactive ? container.scrollTop : frozenScrollTop;
      const plan = buildVisibilityPlan(positions, scrollTop, container.clientHeight, 0);
      const hit = hitTestTile(positions, x, y, textHeight, plan.start, plan.end);
      if (hit !== null && hit < items.length) {
        onTileClick?.(hit, items[hit]);
      }
    };

    baseCanvas.addEventListener('click', handleClick);
    return () => baseCanvas.removeEventListener('click', handleClick);
  }, [frozenScrollTop, interactive, items, onTileClick, textHeight]);

  useEffect(() => () => {
    if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
  }, []);

  return (
    <div
      className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}
      ref={containerRef}
    >
      <div className={styles.canvasWrap} ref={wrapRef}>
        <div className={styles.canvasViewport} ref={viewportRef}>
          <canvas ref={baseCanvasRef} className={styles.baseCanvas} />
        </div>
      </div>
    </div>
  );
}
