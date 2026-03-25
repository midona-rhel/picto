import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSetAtom } from 'jotai';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { gridPerfAtom } from '../../../state/gridPerf';
import type { GridViewMode } from '../layout/types';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { hitTestTile } from './hitTesting';
import { FrameProfiler, Phase } from './frameProfiler';
import { ThumbnailPipeline } from './thumbnailPipeline';
import { drawCanvasBaseLayer } from './drawBase';
import { adaptGridItem } from './renderItemAdapter';
import { buildCanvasVisibilityPlan } from './visibilityPlan';
import {
  CANVAS_SCROLL_IDLE_DELAY_MS,
  classifyCanvasScrollPhase,
  createIdleCanvasScrollState,
  resolveCanvasScrollDirection,
} from './scrollState';
import styles from './CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const PADDING_X = GAP;
const PLACEHOLDER_BG = 'rgba(255, 255, 255, 0.04)';

type DirtyLane = 'base' | 'overlay' | 'both';

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

function diagnosticsEnabled(): boolean {
  if (typeof window === 'undefined') return false;
  const host = window.location.hostname;
  if (host !== '127.0.0.1' && host !== 'localhost') return false;
  try {
    return window.localStorage.getItem('grid-diagnostics') === '1';
  } catch {
    return false;
  }
}

function measureContainerSize(container: HTMLDivElement): { width: number; height: number } {
  const rect = container.getBoundingClientRect();
  const width = container.clientWidth || Math.round(rect.width);
  const height = container.clientHeight || Math.round(rect.height);
  return { width, height };
}

function logCanvasGrid(event: string, data: Record<string, unknown>): void {
  console.warn(`[canvas-grid] ${event}`, data);
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
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const baseContextRef = useRef<CanvasRenderingContext2D | null>(null);
  const overlayContextRef = useRef<CanvasRenderingContext2D | null>(null);
  const rafRef = useRef<number | null>(null);
  const dirtyRef = useRef({ base: true, overlay: true });
  const drawRef = useRef<() => void>(() => {});
  const firstPaintNotifiedRef = useRef(false);
  const scrollStateRef = useRef(createIdleCanvasScrollState());
  const lastScrollTopRef = useRef(0);
  const lastScrollEventAtRef = useRef(0);
  const scrollActiveRef = useRef(false);
  const scrollIdleTimerRef = useRef<number | null>(null);
  const pendingPipelineDirtyRef = useRef(false);
  const backgroundColorRef = useRef('rgb(24, 25, 27)');
  const containerDimsRef = useRef({ width: 0, height: 0 });
  const layoutRef = useRef<ReturnType<typeof computeLayout>>({ positions: [], totalHeight: 0 });
  const targetSizeRef = useRef(targetSize);
  const profilerRef = useRef(new FrameProfiler());
  const visibleTileCountRef = useRef(0);
  const visibleThumbStateCountsRef = useRef({ unique: 0, ready: 0, loading: 0, queued: 0, missing: 0 });
  const evictStateRef = useRef({ lastRun: 0, keepHashes: new Set<string>(), cursor: 0 });
  const drawLogCountRef = useRef(0);
  const scheduleLogCountRef = useRef(0);
  const rafScheduledAtRef = useRef(0);
  const pipelineRef = useRef<ThumbnailPipeline | null>(null);
  const setGridPerf = useSetAtom(gridPerfAtom);
  const perfEnabled = useMemo(diagnosticsEnabled, []);
  const onTileClickRef = useRef(onTileClick);
  const onLoadMoreRef = useRef(onLoadMore);
  const onFirstPaintRef = useRef(onFirstPaint);
  const onScrollTopChangeRef = useRef(onScrollTopChange);

  onTileClickRef.current = onTileClick;
  onLoadMoreRef.current = onLoadMore;
  onFirstPaintRef.current = onFirstPaint;
  onScrollTopChangeRef.current = onScrollTopChange;
  targetSizeRef.current = targetSize;

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;
  const renderItems = useMemo(() => items.map(adaptGridItem), [items]);
  const aspectRatios = useMemo(
    () => renderItems.map((item) => safeAspectRatio(item.aspectRatio ?? 1.5)),
    [renderItems],
  );

  const applyLayout = useCallback((width: number, height: number, scrollbarWidth: number) => {
    layoutRef.current = computeLayout(
      aspectRatios,
      width,
      targetSizeRef.current,
      GAP,
      viewMode,
      textHeight,
      PADDING_X,
      scrollbarWidth,
    );

    if (wrapRef.current) wrapRef.current.style.height = `${layoutRef.current.totalHeight}px`;
    if (viewportRef.current) viewportRef.current.style.height = `${height}px`;

    logCanvasGrid('layout-applied', {
      items: renderItems.length,
      positions: layoutRef.current.positions.length,
      totalHeight: layoutRef.current.totalHeight,
      width,
      height,
      scrollbarWidth,
      viewMode,
      targetSize: targetSizeRef.current,
      textHeight,
    });
  }, [aspectRatios, renderItems.length, textHeight, viewMode]);

  const scheduleRedraw = useCallback(() => {
    if (rafRef.current != null) {
      const ageMs = performance.now() - rafScheduledAtRef.current;
      if (ageMs > 100) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
        if (scheduleLogCountRef.current < 20) {
          logCanvasGrid('schedule-reset-stale-raf', {
            items: renderItems.length,
            baseDirty: dirtyRef.current.base,
            overlayDirty: dirtyRef.current.overlay,
            ageMs,
          });
          scheduleLogCountRef.current += 1;
        }
      } else {
        if (scheduleLogCountRef.current < 20) {
          logCanvasGrid('schedule-skipped-existing-raf', {
            items: renderItems.length,
            baseDirty: dirtyRef.current.base,
            overlayDirty: dirtyRef.current.overlay,
            ageMs,
          });
          scheduleLogCountRef.current += 1;
        }
        return;
      }
    }
    if (scheduleLogCountRef.current < 20) {
      logCanvasGrid('schedule-redraw', {
        items: renderItems.length,
        baseDirty: dirtyRef.current.base,
        overlayDirty: dirtyRef.current.overlay,
      });
      scheduleLogCountRef.current += 1;
    }
    rafScheduledAtRef.current = performance.now();
    rafRef.current = requestAnimationFrame(() => {
      if (scheduleLogCountRef.current < 20) {
        logCanvasGrid('raf-fired', {
          items: renderItems.length,
          baseDirty: dirtyRef.current.base,
          overlayDirty: dirtyRef.current.overlay,
        });
        scheduleLogCountRef.current += 1;
      }
      rafRef.current = null;
      rafScheduledAtRef.current = 0;
      drawRef.current();
    });
  }, [renderItems.length]);

  const markDirty = useCallback((lane: DirtyLane) => {
    if (lane === 'base' || lane === 'both') dirtyRef.current.base = true;
    if (lane === 'overlay' || lane === 'both') dirtyRef.current.overlay = true;
    scheduleRedraw();
  }, [scheduleRedraw]);

  useEffect(() => {
    const pipeline = new ThumbnailPipeline(() => {
      if (scrollActiveRef.current) {
        pendingPipelineDirtyRef.current = true;
        return;
      }
      markDirty('base');
    });
    pipelineRef.current = pipeline;
    return () => {
      pipeline.clear();
      pipelineRef.current = null;
    };
  }, [markDirty]);

  const recomputeLayout = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    const { width, height } = measureContainerSize(container);
    if (width <= 0 || height <= 0) return;
    const scrollbarWidth = container.offsetWidth - width;
    containerDimsRef.current = { width, height };
    applyLayout(width, height, scrollbarWidth);
    markDirty('both');
  }, [applyLayout, markDirty]);

  const ensureCanvasContexts = useCallback((width: number, height: number) => {
    const dpr = window.devicePixelRatio || 1;
    const bufferWidth = Math.ceil(width * dpr);
    const bufferHeight = Math.ceil(height * dpr);

    for (const canvas of [baseCanvasRef.current, overlayCanvasRef.current]) {
      if (!canvas) continue;
      if (canvas.width !== bufferWidth || canvas.height !== bufferHeight) {
        canvas.width = bufferWidth;
        canvas.height = bufferHeight;
        canvas.style.width = `${width}px`;
        canvas.style.height = `${height}px`;
      }
    }

    if (!baseContextRef.current && baseCanvasRef.current) {
      baseContextRef.current = baseCanvasRef.current.getContext('2d', { alpha: true, desynchronized: true });
    }
    if (!overlayContextRef.current && overlayCanvasRef.current) {
      overlayContextRef.current = overlayCanvasRef.current.getContext('2d', { alpha: true, desynchronized: true });
    }

    return { dpr };
  }, []);

  const draw = useCallback(() => {
    const container = containerRef.current;
    const pipeline = pipelineRef.current;
    if (!container || !pipeline) {
      logCanvasGrid('draw-skipped-missing-root', {
        hasContainer: !!container,
        hasPipeline: !!pipeline,
      });
      return;
    }

    let { width, height } = containerDimsRef.current;
    if (width <= 0 || height <= 0) {
      const measured = measureContainerSize(container);
      width = measured.width;
      height = measured.height;
      if (width <= 0 || height <= 0) {
        logCanvasGrid('draw-skipped-zero-size', {
          width,
          height,
          items: renderItems.length,
        });
        return;
      }
      containerDimsRef.current = measured;
      if (viewportRef.current) viewportRef.current.style.height = `${height}px`;
    }
    if (width <= 0 || height <= 0) return;

    const scrollTop = interactive ? container.scrollTop : frozenScrollTop;
    const sizing = ensureCanvasContexts(width, height);
    const baseCtx = baseContextRef.current;
    const overlayCtx = overlayContextRef.current;
    if (!sizing || !baseCtx || !overlayCtx) {
      logCanvasGrid('draw-skipped-missing-context', {
        hasSizing: !!sizing,
        hasBaseCtx: !!baseCtx,
        hasOverlayCtx: !!overlayCtx,
        items: renderItems.length,
        width,
        height,
      });
      return;
    }

    const { dpr } = sizing;
    if (layoutRef.current.positions.length !== renderItems.length) {
      const scrollbarWidth = container.offsetWidth - width;
      applyLayout(width, height, scrollbarWidth);
    }
    const { positions } = layoutRef.current;
    const profiler = profilerRef.current;
    if (perfEnabled) profiler.begin();

    pipeline.setScrollState(scrollStateRef.current);
    const plan = buildCanvasVisibilityPlan({
      positions,
      scrollTop,
      viewportHeight: height,
      scrollPhase: scrollStateRef.current.phase,
      scrollDirection: scrollStateRef.current.direction,
      queueDepth: pipeline.getStats().queueDepth,
    });

    if (renderItems.length > 0 && plan.visibleIterEnd === 0) {
      logCanvasGrid('empty-visible-plan', {
        items: renderItems.length,
        positions: positions.length,
        scrollTop,
        viewportHeight: height,
        startIdx: plan.startIdx,
        endIdx: plan.endIdx,
        totalHeight: layoutRef.current.totalHeight,
      });
    }
    if (drawLogCountRef.current < 20) {
      logCanvasGrid('draw-plan', {
        items: renderItems.length,
        positions: positions.length,
        visibleIterEnd: plan.visibleIterEnd,
        startIdx: plan.startIdx,
        endIdx: plan.endIdx,
        scrollTop,
        viewportHeight: height,
        baseDirty: dirtyRef.current.base,
        overlayDirty: dirtyRef.current.overlay,
      });
    }
    if (perfEnabled) profiler.mark(Phase.visibilityPlan);

    const visibleHashes = new Set<string>();
    let ready = 0;
    let loading = 0;
    let queued = 0;
    let missing = 0;

    for (let n = 0; n < plan.visibleIterEnd; n += 1) {
      const idx = plan.visibleIndices ? plan.visibleIndices[n] : plan.startIdx + n;
      if (idx >= plan.endIdx || idx >= renderItems.length) break;
      const pos = positions[idx];
      const item = renderItems[idx];
      if (!pos || !item || visibleHashes.has(item.thumbnailHash)) continue;
      visibleHashes.add(item.thumbnailHash);
      pipeline.ensure(item.thumbnailHash, {
        y: pos.y + pos.h / 2,
        drawWidth: pos.w,
        drawHeight: pos.h - textHeight,
      });
      const entry = pipeline.get(item.thumbnailHash);
      switch (entry?.state ?? 'idle') {
        case 'shown': ready += 1; break;
        case 'loading': loading += 1; break;
        case 'queued': queued += 1; break;
        default: missing += 1; break;
      }
    }
    if (perfEnabled) profiler.mark(Phase.hashCollection);

    for (const idx of plan.prefetchIndices) {
      const pos = positions[idx];
      const item = renderItems[idx];
      if (!pos || !item) continue;
      pipeline.ensure(item.thumbnailHash, { y: pos.y + pos.h / 2 });
    }
    pipeline.cancelOutsideWindow(plan.cancelTop, plan.cancelBottom);
    if (perfEnabled) profiler.mark(Phase.pipeline);

    const evictState = evictStateRef.current;
    const evictNow = performance.now();
    if (evictNow - evictState.lastRun >= 33) {
      evictState.lastRun = evictNow;
      evictState.keepHashes.clear();
      const keepTop = scrollTop - height;
      const keepBottom = scrollTop + height + height;
      for (let i = Math.max(0, plan.startIdx - 30); i < Math.min(positions.length, plan.endIdx + 30); i += 1) {
        const pos = positions[i];
        const item = renderItems[i];
        if (!pos || !item) continue;
        if (pos.y + pos.h >= keepTop && pos.y <= keepBottom) {
          evictState.keepHashes.add(item.thumbnailHash);
        }
      }
      for (const idx of plan.prefetchIndices) {
        const item = renderItems[idx];
        if (item) evictState.keepHashes.add(item.thumbnailHash);
      }
      const batch = pipeline.getEvictCandidatesBatch(evictState.keepHashes, 5, evictState.cursor);
      evictState.cursor = batch.nextCursor;
      if (batch.evicted.length > 0) {
        pipeline.evictHashes(batch.evicted);
      }
    }

    visibleTileCountRef.current = plan.visibleIterEnd;
    visibleThumbStateCountsRef.current = {
      unique: visibleHashes.size,
      ready,
      loading,
      queued,
      missing,
    };

    if (renderItems.length > 0 && visibleHashes.size === 0) {
      logCanvasGrid('no-visible-hashes', {
        items: renderItems.length,
        positions: positions.length,
        visibleIterEnd: plan.visibleIterEnd,
        startIdx: plan.startIdx,
        endIdx: plan.endIdx,
        scrollTop,
        viewportHeight: height,
      });
    }

    let hasActiveReveal = false;
    if (dirtyRef.current.base) {
      const drawNow = suppressTileReveal ? Number.MAX_SAFE_INTEGER : performance.now();
      baseCtx.setTransform(1, 0, 0, 1, 0, 0);
      baseCtx.fillStyle = backgroundColorRef.current;
      baseCtx.fillRect(0, 0, baseCanvasRef.current?.width ?? 0, baseCanvasRef.current?.height ?? 0);
      baseCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
      if (perfEnabled) profiler.mark(Phase.clear);

      hasActiveReveal = drawCanvasBaseLayer({
        ctx: baseCtx,
        positions,
        items: renderItems,
        atlasGet: (hash) => pipeline.get(hash),
        atlasEnsure: (hash, args) => pipeline.ensure(hash, args),
        now: drawNow,
        visible: {
          startIdx: plan.startIdx,
          endIdx: plan.endIdx,
          visibleIndices: plan.visibleIndices,
          visibleIterEnd: plan.visibleIterEnd,
          scrollTop,
          cssH: height,
          th: textHeight,
          br: 8,
        },
        theme: {
          placeholderBg: PLACEHOLDER_BG,
          borderRadius: 8,
        },
        viewMode,
        showTileName: showName,
        showExtension,
      });
      if (perfEnabled) {
        profiler.mark(Phase.imageDraw);
        profiler.mark(Phase.chromeDraw);
      }
      if (drawLogCountRef.current < 20) {
        logCanvasGrid('draw-complete', {
          items: renderItems.length,
          visibleIterEnd: plan.visibleIterEnd,
          visibleThumbs: visibleHashes.size,
          ready,
          loading,
          queued,
          missing,
          canvasWidth: baseCanvasRef.current?.width ?? 0,
          canvasHeight: baseCanvasRef.current?.height ?? 0,
          cssWidth: width,
          cssHeight: height,
          hasActiveReveal,
        });
        drawLogCountRef.current += 1;
      }
      dirtyRef.current.base = false;
    }

    if (dirtyRef.current.overlay) {
      overlayCtx.setTransform(1, 0, 0, 1, 0, 0);
      overlayCtx.clearRect(0, 0, overlayCanvasRef.current?.width ?? 0, overlayCanvasRef.current?.height ?? 0);
      dirtyRef.current.overlay = false;
    }

    if (!firstPaintNotifiedRef.current && items.length > 0 && plan.visibleIterEnd > 0) {
      firstPaintNotifiedRef.current = true;
      onFirstPaintRef.current?.();
    }

    if (perfEnabled) {
      profiler.end({
        visibleTiles: visibleTileCountRef.current,
        expectContinuousFrames: hasActiveReveal,
      });
    }

    if (hasActiveReveal) {
      dirtyRef.current.base = true;
      scheduleRedraw();
    }
  }, [
    ensureCanvasContexts,
    frozenScrollTop,
    interactive,
    items.length,
    perfEnabled,
    renderItems,
    scheduleRedraw,
    showExtension,
    showName,
    suppressTileReveal,
    textHeight,
    viewMode,
  ]);

  drawRef.current = draw;

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !interactive) return;

    const handleScroll = () => {
      const scrollTop = container.scrollTop;
      const previous = lastScrollTopRef.current;
      const now = performance.now();
      const delta = scrollTop - previous;
      const elapsed = lastScrollEventAtRef.current > 0 ? now - lastScrollEventAtRef.current : 0;
      const velocityPxPerSec = elapsed > 0 ? (Math.abs(delta) / elapsed) * 1000 : 0;

      lastScrollTopRef.current = scrollTop;
      lastScrollEventAtRef.current = now;
      scrollStateRef.current = {
        phase: classifyCanvasScrollPhase(velocityPxPerSec),
        direction: resolveCanvasScrollDirection(delta),
        velocityPxPerSec,
      };
      scrollActiveRef.current = true;
      onScrollTopChangeRef.current?.(scrollTop);
      markDirty('base');

      if (scrollIdleTimerRef.current != null) window.clearTimeout(scrollIdleTimerRef.current);
      scrollIdleTimerRef.current = window.setTimeout(() => {
        scrollIdleTimerRef.current = null;
        scrollActiveRef.current = false;
        scrollStateRef.current = createIdleCanvasScrollState();
        if (pendingPipelineDirtyRef.current) {
          pendingPipelineDirtyRef.current = false;
          markDirty('base');
        }
      }, CANVAS_SCROLL_IDLE_DELAY_MS);

      const loadMore = onLoadMoreRef.current;
      if (loadMore) {
        const { scrollHeight, clientHeight } = container;
        if (scrollHeight - scrollTop - clientHeight < 400) {
          loadMore();
        }
      }
    };

    container.addEventListener('scroll', handleScroll, { passive: true });
    return () => {
      container.removeEventListener('scroll', handleScroll);
      if (scrollIdleTimerRef.current != null) window.clearTimeout(scrollIdleTimerRef.current);
    };
  }, [interactive, markDirty]);

  useEffect(() => {
    if (!interactive) {
      lastScrollTopRef.current = frozenScrollTop;
      scrollStateRef.current = createIdleCanvasScrollState();
      markDirty('base');
    }
  }, [frozenScrollTop, interactive, markDirty]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    backgroundColorRef.current = getComputedStyle(container).backgroundColor || backgroundColorRef.current;
    const observer = new ResizeObserver(() => {
      backgroundColorRef.current = getComputedStyle(container).backgroundColor || backgroundColorRef.current;
      recomputeLayout();
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, [recomputeLayout]);

  useEffect(() => {
    firstPaintNotifiedRef.current = false;
    logCanvasGrid('items-updated', {
      items: items.length,
      renderItems: renderItems.length,
      viewMode,
      targetSize,
      showName,
      showExtension,
    });
    recomputeLayout();
    markDirty('both');
  }, [items, markDirty, recomputeLayout, renderItems.length, showExtension, showName, targetSize, viewMode]);

  useEffect(() => {
    const container = containerRef.current;
    const canvas = baseCanvasRef.current;
    if (!container || !canvas) return;

    const handleClick = (event: MouseEvent) => {
      const rect = container.getBoundingClientRect();
      const scrollTop = interactive ? container.scrollTop : frozenScrollTop;
      const x = event.clientX - rect.left;
      const y = event.clientY - rect.top + scrollTop;
      const { positions } = layoutRef.current;
      const plan = buildCanvasVisibilityPlan({
        positions,
        scrollTop,
        viewportHeight: container.clientHeight,
        scrollPhase: scrollStateRef.current.phase,
        scrollDirection: scrollStateRef.current.direction,
        queueDepth: pipelineRef.current?.getStats().queueDepth ?? 0,
      });
      const hit = hitTestTile(positions, x, y, textHeight, plan.startIdx, plan.endIdx);
      if (hit !== null && hit < items.length) {
        onTileClickRef.current?.(hit, items[hit]);
      }
    };

    canvas.addEventListener('click', handleClick);
    return () => canvas.removeEventListener('click', handleClick);
  }, [frozenScrollTop, interactive, items, textHeight]);

  useEffect(() => () => {
    if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
  }, []);

  const [profilerText, setProfilerText] = useState('');
  useEffect(() => {
    if (!perfEnabled || !interactive) {
      setGridPerf(null);
      return;
    }

    const publish = () => {
      const frameStats = profilerRef.current.getStats();
      const pipelineStats = pipelineRef.current?.getStats();
      const thumbStates = visibleThumbStateCountsRef.current;
      const nonTotalPhases = frameStats.phases
        .filter((phase) => phase.name !== 'TOTAL')
        .sort((a, b) => b.p99 - a.p99);

      setGridPerf({
        fps: frameStats.fps,
        droppedFrames: frameStats.droppedFrames,
        nearThresholdFrames: frameStats.nearThresholdFrames,
        missedFrames: frameStats.missedFrames,
        pauseFrames: frameStats.pauseFrames,
        drawOverBudgetFrames: frameStats.drawOverBudgetFrames,
        avgFrameGapMs: frameStats.avgFrameGapMs,
        maxFrameGapMs: frameStats.maxFrameGapMs,
        maxMissedFrameGapMs: frameStats.maxMissedFrameGapMs,
        maxPauseGapMs: frameStats.maxPauseGapMs,
        avgRafDelayMs: frameStats.avgRafDelayMs,
        maxRafDelayMs: frameStats.maxRafDelayMs,
        totalP99Ms: frameStats.phases.find((phase) => phase.name === 'TOTAL')?.p99 ?? 0,
        slowestPhase: nonTotalPhases[0]?.name ?? 'none',
        slowestPhaseP99Ms: nonTotalPhases[0]?.p99 ?? 0,
        queueDepth: pipelineStats?.queueDepth ?? 0,
        activeLoads: pipelineStats?.activeLoads ?? 0,
        cacheEntries: pipelineStats?.cacheEntries ?? 0,
        cacheMb: (pipelineStats?.totalBytes ?? 0) / (1024 * 1024),
        visibleTileCount: visibleTileCountRef.current,
        visibleUniqueThumbCount: thumbStates.unique,
        visibleUniqueThumbReady: thumbStates.ready,
        visibleUniqueThumbLoading: thumbStates.loading,
        visibleUniqueThumbQueued: thumbStates.queued,
        visibleUniqueThumbMissing: thumbStates.missing,
        scrollActive: scrollActiveRef.current,
        scrollFrames: 0,
        avgScrollVelocityPxPerMs: scrollStateRef.current.velocityPxPerSec / 1000,
        maxScrollVelocityPxPerMs: scrollStateRef.current.velocityPxPerSec / 1000,
        rafFramesWhileIdle: 0,
        rafFramesWhileScrolling: 0,
        scrollTranslationMode: 'unsnapped',
        inferredCause: scrollActiveRef.current ? 'presentation_bound' : 'idle_noise',
        inferredReason: scrollActiveRef.current
          ? 'Legacy-style scroll scheduling is active; diagnostics are opt-in to stay out of the hot path.'
          : 'Diagnostics are sampled outside the hot path and only reflect explicit debug sessions.',
        updatedAt: performance.now(),
      });
      setProfilerText(profilerRef.current.formatStats());
    };

    publish();
    const timer = window.setInterval(publish, 500);
    return () => {
      window.clearInterval(timer);
      setGridPerf(null);
    };
  }, [interactive, perfEnabled, setGridPerf]);

  return (
    <div
      className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}
      ref={containerRef}
    >
      <div className={styles.canvasWrap} ref={wrapRef}>
        <div className={styles.canvasViewport} ref={viewportRef}>
          <canvas ref={baseCanvasRef} className={styles.baseCanvas} />
          <canvas ref={overlayCanvasRef} className={styles.overlayCanvas} />
        </div>
      </div>
      {perfEnabled && interactive && profilerText && (
        <pre className={styles.profilerOverlay}>{profilerText}</pre>
      )}
    </div>
  );
}
