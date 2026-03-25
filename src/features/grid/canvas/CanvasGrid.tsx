/**
 * Canvas grid — single-canvas renderer with visibility-based thumbnail loading.
 */

import { useEffect, useRef, useCallback, useMemo, useState } from 'react';
import { useSetAtom } from 'jotai';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode } from '../layout/types';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { buildVisibilityPlan } from './visibilityPlan';
import { drawTileChromeLayer, drawTileMediaLayer } from './drawBase';
import { hitTestTile } from './hitTesting';
import { ThumbnailPipeline, type ImageRequest } from './thumbnailPipeline';
import { FrameProfiler, Phase } from './frameProfiler';
import { gridPerfAtom } from '../../../state/gridPerf';
import styles from './CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const PADDING_X = GAP;
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

function inferGridCause(input: {
  frameStats: ReturnType<FrameProfiler['getStats']>;
  slowestPhase: string;
  slowestPhaseP99Ms: number;
  queueDepth: number;
  activeLoads: number;
  visibleTileCount: number;
  visibleUniqueThumbCount: number;
  visibleUniqueThumbReady: number;
  visibleUniqueThumbLoading: number;
  visibleUniqueThumbQueued: number;
  visibleUniqueThumbMissing: number;
  scrollActive: boolean;
  scrollFrames: number;
}): { cause: string; reason: string } {
  const {
    frameStats,
    slowestPhase,
    slowestPhaseP99Ms,
    queueDepth,
    activeLoads,
    visibleTileCount,
    visibleUniqueThumbCount,
    visibleUniqueThumbReady,
    visibleUniqueThumbLoading,
    visibleUniqueThumbQueued,
    visibleUniqueThumbMissing,
    scrollActive,
    scrollFrames,
  } = input;

  const visiblePending = visibleUniqueThumbLoading + visibleUniqueThumbQueued + visibleUniqueThumbMissing;
  if (visiblePending > 0 && activeLoads >= 4) {
    return {
      cause: 'pipeline_bound',
      reason: `${visiblePending}/${visibleUniqueThumbCount} visible thumbnails are not ready while ${activeLoads} loads are active and queue depth is ${queueDepth}.`,
    };
  }

  if (slowestPhase === 'pipeline' && slowestPhaseP99Ms > 2) {
    return {
      cause: 'pipeline_bound',
      reason: `Pipeline work is the slowest phase at p99 ${slowestPhaseP99Ms.toFixed(2)}ms with queue depth ${queueDepth}.`,
    };
  }

  if ((slowestPhase === 'images' || slowestPhase === 'chrome') && slowestPhaseP99Ms > 4) {
    return {
      cause: 'draw_bound',
      reason: `Canvas draw is the slowest phase at p99 ${slowestPhaseP99Ms.toFixed(2)}ms with ${visibleTileCount} visible tiles.`,
    };
  }

  if ((slowestPhase === 'visibility' || slowestPhase === 'hashes' || slowestPhase === 'pipeline') && slowestPhaseP99Ms > 2) {
    return {
      cause: 'prep_bound',
      reason: `${slowestPhase} work is the slowest phase at p99 ${slowestPhaseP99Ms.toFixed(2)}ms before the draw even begins.`,
    };
  }

  if (
    visibleUniqueThumbReady === visibleUniqueThumbCount
    && frameStats.drawOverBudgetFrames === 0
    && frameStats.missedFrames > 0
  ) {
    if (scrollActive || scrollFrames > 0) {
      return {
        cause: 'presentation_bound',
        reason: 'Visible thumbnails are already ready and draw is within budget, so the missed frames are happening in scroll/presentation timing rather than canvas raster cost.',
      };
    }
    return {
      cause: 'idle_noise',
      reason: 'The rolling window still contains earlier presentation misses, but the grid is currently idle and not draw-bound.',
    };
  }

  if (frameStats.missedFrames === 0 && frameStats.nearThresholdFrames > 0) {
    return {
      cause: 'idle_noise',
      reason: 'The grid is hovering near the 120Hz budget without producing real missed frames.',
    };
  }

  return {
    cause: 'idle_noise',
    reason: `No single dominant cause. Slowest phase is ${slowestPhase} at p99 ${slowestPhaseP99Ms.toFixed(2)}ms.`,
  };
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
  const scrollActiveRef = useRef(false);
  const scrollIdleTimerRef = useRef<number | null>(null);
  const backgroundColorRef = useRef<string>('rgb(24, 25, 27)');
  const onScrollTopChangeRef = useRef(onScrollTopChange);
  onScrollTopChangeRef.current = onScrollTopChange;
  const onLoadMoreRef = useRef(onLoadMore);
  onLoadMoreRef.current = onLoadMore;
  const onTileClickRef = useRef(onTileClick);
  onTileClickRef.current = onTileClick;
  const onFirstPaintRef = useRef(onFirstPaint);
  onFirstPaintRef.current = onFirstPaint;
  const profilerRef = useRef(new FrameProfiler());
  const lastPlanRef = useRef({ start: -1, end: -1 });
  const visibleTileCountRef = useRef(0);
  const visibleThumbStateCountsRef = useRef({ unique: 0, ready: 0, loading: 0, queued: 0, missing: 0 });
  const setGridPerf = useSetAtom(gridPerfAtom);
  const lastLoggedPerfRef = useRef({
    missedFrames: 0,
    drawOverBudgetFrames: 0,
    cause: '',
    fpsBucket: 120,
  });
  // Cached container dimensions — only updated on resize, never read from DOM per frame
  const containerDimsRef = useRef({ width: 0, height: 0 });
  // Scratch buffers reused every frame to avoid allocations
  const scratchVisible = useRef<ImageRequest[]>([]);
  const scratchAhead = useRef<ImageRequest[]>([]);
  const scratchBehind = useRef<ImageRequest[]>([]);
  const scratchSeen = useRef<Set<string>>(new Set());
  const scratchRevealProgress = useRef<Map<string, number>>(new Map());
  const scratchAheadIdx = useRef<number[]>([]);
  const scratchBehindIdx = useRef<number[]>([]);
  const rafScheduledAtRef = useRef(0);
  const lastScrollEventAtRef = useRef(0);
  const telemetryRef = useRef({
    scrollEvents: 0,
    scrollFrames: 0,
    rafScheduled: 0,
    rafExecuted: 0,
    framesDrawn: 0,
    framesSkipped: 0,
    rafFramesWhileIdle: 0,
    rafFramesWhileScrolling: 0,
    scrollVelocitySum: 0,
    scrollVelocitySamples: 0,
    maxScrollVelocity: 0,
  });

  const requestProfiledFrame = useCallback((cb: () => void): number => {
    rafScheduledAtRef.current = performance.now();
    telemetryRef.current.rafScheduled++;
    return requestAnimationFrame(() => {
      telemetryRef.current.rafExecuted++;
      if (scrollActiveRef.current) {
        telemetryRef.current.rafFramesWhileScrolling++;
      } else {
        telemetryRef.current.rafFramesWhileIdle++;
      }
      profilerRef.current.noteRafDelay(performance.now() - rafScheduledAtRef.current);
      cb();
    });
  }, []);

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;

  const scheduleRedraw = useCallback(() => {
    if (rafRef.current != null) return;
    rafRef.current = requestProfiledFrame(() => {
      rafRef.current = null;
      drawRef.current();
    });
  }, [requestProfiledFrame]);

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
    const height = container.clientHeight;
    const scrollbarW = container.offsetWidth - width;
    containerDimsRef.current = { width, height };

    layoutRef.current = computeLayout(aspectRatios, width, targetSizeRef.current, GAP, viewMode, textHeight, PADDING_X, scrollbarW);

    if (wrapRef.current) wrapRef.current.style.height = `${layoutRef.current.totalHeight}px`;
    if (viewportRef.current) viewportRef.current.style.height = `${height}px`;

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
    const profiler = profilerRef.current;
    const container = containerRef.current;
    const baseCanvas = baseCanvasRef.current;
    const pipeline = pipelineRef.current;
    if (!container || !baseCanvas || !pipeline) return;

    profiler.begin();

    const dpr = window.devicePixelRatio || 1;
    const { width, height } = containerDimsRef.current;
    if (width === 0 || height === 0) { profiler.end(); return; }
    const scrollTop = interactive ? container.scrollTop : frozenScrollTop;

    const pixelWidth = Math.ceil(width * dpr);
    const pixelHeight = Math.ceil(height * dpr);
    if (baseCanvas.width !== pixelWidth || baseCanvas.height !== pixelHeight) {
      baseCanvas.width = pixelWidth;
      baseCanvas.height = pixelHeight;
      baseCanvas.style.width = `${width}px`;
      baseCanvas.style.height = `${height}px`;
      baseContextRef.current = baseCanvas.getContext('2d', { alpha: false });
      dirtyRef.current = true;
    }

    const ctx = baseContextRef.current;
    if (!ctx) { profiler.end(); return; }

    const { positions } = layoutRef.current;
    const plan = buildVisibilityPlan(
      positions, scrollTop, height, scrollDirectionRef.current,
      scratchAheadIdx.current, scratchBehindIdx.current,
    );
    visibleTileCountRef.current = Math.max(0, plan.end - plan.start);
    profiler.mark(Phase.visibilityPlan);

    const visibleReqs = scratchVisible.current; visibleReqs.length = 0;
    const aheadReqs = scratchAhead.current; aheadReqs.length = 0;
    const behindReqs = scratchBehind.current; behindReqs.length = 0;
    const seen = scratchSeen.current; seen.clear();
    const visibleHashes: string[] = [];
    let visibleUniqueThumbReady = 0;
    let visibleUniqueThumbLoading = 0;
    let visibleUniqueThumbQueued = 0;
    let visibleUniqueThumbMissing = 0;

    for (let i = plan.start; i < plan.end && i < items.length; i++) {
      const hash = items[i].thumbnail_hash;
      if (!hash || seen.has(hash)) continue;
      seen.add(hash);
      const pos = positions[i];
      visibleHashes.push(hash);
      switch (pipeline.getState(hash)) {
        case 'ready': visibleUniqueThumbReady++; break;
        case 'loading': visibleUniqueThumbLoading++; break;
        case 'queued': visibleUniqueThumbQueued++; break;
        case 'missing': visibleUniqueThumbMissing++; break;
      }
      visibleReqs.push({ hash, displayWidth: pos ? pos.w | 0 : 0, displayHeight: pos ? (pos.h - textHeight) | 0 : 0 });
    }
    for (const idx of plan.aheadPrefetchIndices) {
      if (idx < 0 || idx >= items.length) continue;
      const hash = items[idx].thumbnail_hash;
      if (!hash || seen.has(hash)) continue;
      seen.add(hash);
      const pos = positions[idx];
      aheadReqs.push({ hash, displayWidth: pos ? pos.w | 0 : 0, displayHeight: pos ? (pos.h - textHeight) | 0 : 0 });
    }
    for (const idx of plan.behindPrefetchIndices) {
      if (idx < 0 || idx >= items.length) continue;
      const hash = items[idx].thumbnail_hash;
      if (!hash || seen.has(hash)) continue;
      seen.add(hash);
      const pos = positions[idx];
      behindReqs.push({ hash, displayWidth: pos ? pos.w | 0 : 0, displayHeight: pos ? (pos.h - textHeight) | 0 : 0 });
    }
    profiler.mark(Phase.hashCollection);
    visibleThumbStateCountsRef.current = {
      unique: visibleHashes.length,
      ready: visibleUniqueThumbReady,
      loading: visibleUniqueThumbLoading,
      queued: visibleUniqueThumbQueued,
      missing: visibleUniqueThumbMissing,
    };

    const planChanged = plan.start !== lastPlanRef.current.start || plan.end !== lastPlanRef.current.end;
    if (planChanged) {
      lastPlanRef.current = { start: plan.start, end: plan.end };
      pipeline.request({ visible: visibleReqs, ahead: aheadReqs, behind: behindReqs });
      pipeline.evictExcept(seen);
    }
    profiler.mark(Phase.pipeline);

    // Single-pass reveal progress computation
    const revealProgressByHash = scratchRevealProgress.current; revealProgressByHash.clear();
    let needsNextAnimationFrame = false;

    if (suppressTileReveal) {
      for (const hash of visibleHashes) revealProgressByHash.set(hash, 1);
      lastVisibleSetRef.current = new Set(visibleHashes);
    } else {
      const now = performance.now();
      const prevVisible = lastVisibleSetRef.current;

      for (const hash of visibleHashes) {
        if (!prevVisible.has(hash)) {
          revealStatesRef.current.set(hash, { startAt: nextRevealSlot(now) });
        }
        const state = revealStatesRef.current.get(hash);
        if (!state || !pipeline.get(hash)) {
          revealProgressByHash.set(hash, 1);
        } else {
          const progress = Math.min(1, (now - state.startAt) / REVEAL_DURATION_MS);
          revealProgressByHash.set(hash, progress);
          if (progress < 1) needsNextAnimationFrame = true;
        }
      }

      for (const hash of revealStatesRef.current.keys()) {
        if (!seen.has(hash)) revealStatesRef.current.delete(hash);
      }

      let changed = visibleHashes.length !== prevVisible.size;
      if (!changed) {
        for (const h of visibleHashes) { if (!prevVisible.has(h)) { changed = true; break; } }
      }
      if (changed) lastVisibleSetRef.current = new Set(visibleHashes);
    }
    profiler.mark(Phase.revealCompute);

    if (dirtyRef.current || needsNextAnimationFrame) {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = backgroundColorRef.current;
      ctx.fillRect(0, 0, baseCanvas.width, baseCanvas.height);
      ctx.setTransform(dpr, 0, 0, dpr, 0, -(scrollTop * dpr));
      profiler.mark(Phase.clear);

      drawTileMediaLayer({
        ctx, items, positions,
        thumbnails: pipeline.getAll(),
        revealProgressByHash, textHeight,
        visibleStart: plan.start, visibleEnd: plan.end,
        showName, showExtension,
      });
      profiler.mark(Phase.imageDraw);

      drawTileChromeLayer({
        ctx, items, positions,
        thumbnails: pipeline.getAll(),
        revealProgressByHash, textHeight,
        visibleStart: plan.start, visibleEnd: plan.end,
        showName, showExtension,
      });
      profiler.mark(Phase.chromeDraw);

      ctx.setTransform(1, 0, 0, 1, 0, 0);
      dirtyRef.current = false;
      telemetryRef.current.framesDrawn++;
      if (scrollActiveRef.current) {
        telemetryRef.current.scrollFrames++;
      }

      if (!firstPaintNotifiedRef.current && items.length > 0 && plan.end > plan.start) {
        firstPaintNotifiedRef.current = true;
        onFirstPaintRef.current?.();
      }
    } else {
      telemetryRef.current.framesSkipped++;
    }

    profiler.end({
      visibleTiles: visibleTileCountRef.current,
      expectContinuousFrames: needsNextAnimationFrame,
    });

    if (needsNextAnimationFrame) {
      dirtyRef.current = true;
      rafRef.current = requestProfiledFrame(() => {
        rafRef.current = null;
        drawRef.current();
      });
    }
  }, [
    items,
    frozenScrollTop,
    interactive,
    scheduleRedraw,
    showExtension,
    showName,
    suppressTileReveal,
    textHeight,
    nextRevealSlot,
    requestProfiledFrame,
  ]);

  drawRef.current = draw;

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !interactive) return;

    const SCROLL_IDLE_MS = 150;

    const handleScroll = () => {
      telemetryRef.current.scrollEvents++;
      const scrollTop = container.scrollTop;
      const previous = lastScrollTopRef.current;
      const now = performance.now();
      const dt = lastScrollEventAtRef.current > 0 ? now - lastScrollEventAtRef.current : 0;
      if (dt > 0) {
        const velocity = Math.abs(scrollTop - previous) / dt;
        telemetryRef.current.scrollVelocitySum += velocity;
        telemetryRef.current.scrollVelocitySamples++;
        if (velocity > telemetryRef.current.maxScrollVelocity) {
          telemetryRef.current.maxScrollVelocity = velocity;
        }
      }
      lastScrollEventAtRef.current = now;
      scrollDirectionRef.current = scrollTop > previous ? 1 : scrollTop < previous ? -1 : 0;
      lastScrollTopRef.current = scrollTop;
      onScrollTopChangeRef.current?.(scrollTop);
      dirtyRef.current = true;

      scrollActiveRef.current = true;
      if (scrollIdleTimerRef.current != null) clearTimeout(scrollIdleTimerRef.current);
      scrollIdleTimerRef.current = window.setTimeout(() => {
        scrollIdleTimerRef.current = null;
        scrollActiveRef.current = false;
      }, SCROLL_IDLE_MS);

      scheduleRedraw();

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
      if (scrollIdleTimerRef.current != null) clearTimeout(scrollIdleTimerRef.current);
    };
  }, [interactive, scheduleRedraw]); // callbacks via refs — no effect churn

  // Read background color once on mount and on resize (proxy for theme change)
  useEffect(() => {
    const container = containerRef.current;
    if (container) {
      backgroundColorRef.current = getComputedStyle(container).backgroundColor || backgroundColorRef.current;
    }
  }, []);

  // Debounced resize — fire immediately on first observation, debounce subsequent
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let timer: number | null = null;
    let firstFire = true;
    const DEBOUNCE = 150;

    const ro = new ResizeObserver(() => {
      if (firstFire) {
        firstFire = false;
        backgroundColorRef.current = getComputedStyle(container).backgroundColor || backgroundColorRef.current;
        recomputeLayout();
        return;
      }
      if (timer != null) clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = null;
        backgroundColorRef.current = getComputedStyle(container).backgroundColor || backgroundColorRef.current;
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
        onTileClickRef.current?.(hit, items[hit]);
      }
    };

    baseCanvas.addEventListener('click', handleClick);
    return () => baseCanvas.removeEventListener('click', handleClick);
  }, [frozenScrollTop, interactive, items, textHeight]);

  useEffect(() => () => {
    if (rafRef.current != null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
  }, []);

  // Dev overlay — shows frame profiler stats, updates every 500ms (doesn't affect measurements)
  const isDevHost = typeof window !== 'undefined'
    && (window.location.hostname === '127.0.0.1' || window.location.hostname === 'localhost');
  const [profilerText, setProfilerText] = useState('');
  useEffect(() => {
    if (!isDevHost || !interactive) return;
    const timer = window.setInterval(() => {
      setProfilerText(profilerRef.current.formatStats());
    }, 500);
    return () => window.clearInterval(timer);
  }, [isDevHost, interactive]);

  useEffect(() => {
    if (!interactive) {
      setGridPerf(null);
      return;
    }

    const publish = () => {
      const frameStats = profilerRef.current.getStats();
      const pipelineStats = pipelineRef.current?.getStats();
      const visibleThumbStates = visibleThumbStateCountsRef.current;
      const telemetry = telemetryRef.current;
      const slowestPhase = frameStats.phases
        .filter((phase) => phase.name !== 'TOTAL')
        .sort((a, b) => b.p99 - a.p99)[0];
      const inferred = inferGridCause({
        frameStats,
        slowestPhase: slowestPhase?.name ?? 'none',
        slowestPhaseP99Ms: slowestPhase?.p99 ?? 0,
        queueDepth: pipelineStats?.queueDepth ?? 0,
        activeLoads: pipelineStats?.activeLoads ?? 0,
        visibleTileCount: visibleTileCountRef.current,
        visibleUniqueThumbCount: visibleThumbStates.unique,
        visibleUniqueThumbReady: visibleThumbStates.ready,
        visibleUniqueThumbLoading: visibleThumbStates.loading,
        visibleUniqueThumbQueued: visibleThumbStates.queued,
        visibleUniqueThumbMissing: visibleThumbStates.missing,
        scrollActive: scrollActiveRef.current,
        scrollFrames: telemetry.scrollFrames,
      });

      const fpsBucket = frameStats.fps >= 110 ? 120 : frameStats.fps >= 80 ? 90 : frameStats.fps >= 50 ? 60 : 30;
      const shouldLog =
        frameStats.missedFrames > lastLoggedPerfRef.current.missedFrames
        || frameStats.drawOverBudgetFrames > lastLoggedPerfRef.current.drawOverBudgetFrames
        || inferred.cause !== lastLoggedPerfRef.current.cause
        || fpsBucket < lastLoggedPerfRef.current.fpsBucket;

      if (shouldLog) {
        if (frameStats.missedFrames > lastLoggedPerfRef.current.missedFrames) {
          console.warn(
            `[grid-perf] missed-frame cause=${inferred.cause} gap=${frameStats.maxMissedFrameGapMs.toFixed(2)}ms draw=${(frameStats.phases.find((phase) => phase.name === 'TOTAL')?.p99 ?? 0).toFixed(2)}ms visibleTiles=${visibleTileCountRef.current} visibleThumbs=${visibleThumbStates.unique} queue=${pipelineStats?.queueDepth ?? 0} loads=${pipelineStats?.activeLoads ?? 0}`,
          );
        }
        if (
          frameStats.drawOverBudgetFrames > lastLoggedPerfRef.current.drawOverBudgetFrames
          || inferred.cause !== lastLoggedPerfRef.current.cause
          || fpsBucket < lastLoggedPerfRef.current.fpsBucket
        ) {
          console.warn(
            `[grid-perf] snapshot fps=${frameStats.fps} missed=${frameStats.missedFrames} near=${frameStats.nearThresholdFrames} pauses=${frameStats.pauseFrames} drawOver=${frameStats.drawOverBudgetFrames} totalP99=${(frameStats.phases.find((phase) => phase.name === 'TOTAL')?.p99 ?? 0).toFixed(2)}ms cause=${inferred.cause} reason=${inferred.reason} visibleTiles=${visibleTileCountRef.current} visibleThumbs=${visibleThumbStates.unique} ready=${visibleThumbStates.ready} loading=${visibleThumbStates.loading} queued=${visibleThumbStates.queued} missing=${visibleThumbStates.missing} queue=${pipelineStats?.queueDepth ?? 0} loads=${pipelineStats?.activeLoads ?? 0} scrollActive=${scrollActiveRef.current} scrollFrames=${telemetry.scrollFrames}`,
          );
        }
        lastLoggedPerfRef.current = {
          missedFrames: frameStats.missedFrames,
          drawOverBudgetFrames: frameStats.drawOverBudgetFrames,
          cause: inferred.cause,
          fpsBucket,
        };
      }

      const avgScrollVelocityPxPerMs = telemetry.scrollVelocitySamples > 0
        ? telemetry.scrollVelocitySum / telemetry.scrollVelocitySamples
        : 0;

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
        slowestPhase: slowestPhase?.name ?? 'none',
        slowestPhaseP99Ms: slowestPhase?.p99 ?? 0,
        queueDepth: pipelineStats?.queueDepth ?? 0,
        activeLoads: pipelineStats?.activeLoads ?? 0,
        cacheEntries: pipelineStats?.cacheEntries ?? 0,
        cacheMb: ((pipelineStats?.totalBytes ?? 0) / (1024 * 1024)),
        visibleTileCount: visibleTileCountRef.current,
        visibleUniqueThumbCount: visibleThumbStates.unique,
        visibleUniqueThumbReady: visibleThumbStates.ready,
        visibleUniqueThumbLoading: visibleThumbStates.loading,
        visibleUniqueThumbQueued: visibleThumbStates.queued,
        visibleUniqueThumbMissing: visibleThumbStates.missing,
        scrollActive: scrollActiveRef.current,
        scrollFrames: telemetry.scrollFrames,
        avgScrollVelocityPxPerMs,
        maxScrollVelocityPxPerMs: telemetry.maxScrollVelocity,
        rafFramesWhileIdle: telemetry.rafFramesWhileIdle,
        rafFramesWhileScrolling: telemetry.rafFramesWhileScrolling,
        scrollTranslationMode: 'unsnapped',
        inferredCause: inferred.cause,
        inferredReason: inferred.reason,
        updatedAt: performance.now(),
      });

      telemetry.scrollEvents = 0;
      telemetry.scrollFrames = 0;
      telemetry.rafScheduled = 0;
      telemetry.rafExecuted = 0;
      telemetry.framesDrawn = 0;
      telemetry.framesSkipped = 0;
      telemetry.rafFramesWhileIdle = 0;
      telemetry.rafFramesWhileScrolling = 0;
      telemetry.scrollVelocitySum = 0;
      telemetry.scrollVelocitySamples = 0;
      telemetry.maxScrollVelocity = 0;
    };

    publish();
    const timer = window.setInterval(publish, 250);
    return () => {
      window.clearInterval(timer);
      setGridPerf(null);
    };
  }, [interactive, setGridPerf]);

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
      {isDevHost && interactive && profilerText && (
        <pre className={styles.profilerOverlay}>{profilerText}</pre>
      )}
    </div>
  );
}
