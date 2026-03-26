import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSetAtom } from 'jotai';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { gridPerfAtom } from '../../../state/gridPerf';
import { gridFrameTraceAtom } from '../../../state/gridTrace';
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
import {
  type GridFrameTrace,
  type GridTraceCause,
  type GridTraceEvent,
  type GridTraceStatus,
  classifyTraceStatus,
  createTraceEvent,
  gridTraceRecorder,
  inferTraceCause,
} from './gridTrace';
import styles from './CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const PADDING_X = GAP;
const PLACEHOLDER_BG = 'rgba(255, 255, 255, 0.04)';
const SCROLL_VISIBLE_PROMOTION_BUDGET = 2;
const IDLE_VISIBLE_PROMOTION_BUDGET = 24;
const IDLE_PREFETCH_PROMOTION_BUDGET = 12;

type DirtyLane = 'base' | 'overlay' | 'both';

interface PendingTraceEvent {
  createdAt: number;
  type: string;
  payload?: Record<string, unknown>;
}

interface PendingScheduleTrace {
  reasons: string[];
  requestedAt: number | null;
  firedAt: number | null;
  hadPendingRaf: boolean;
  staleReset: boolean;
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
  const lastPresentedScrollTopRef = useRef<number | null>(null);
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
  const rafScheduledAtRef = useRef(0);
  const rafFiredAtRef = useRef<number | null>(null);
  const pendingPromotionHashesRef = useRef<Set<string>>(new Set());
  const scrollEventCountRef = useRef(0);
  const rafScheduledCountRef = useRef(0);
  const rafExecutedCountRef = useRef(0);
  const drawnFrameCountRef = useRef(0);
  const skippedFrameCountRef = useRef(0);
  const scrollFramesRef = useRef(0);
  const rafFramesWhileIdleRef = useRef(0);
  const rafFramesWhileScrollingRef = useRef(0);
  const scrollVelocitySamplesRef = useRef({ sum: 0, count: 0, max: 0 });
  const pipelineRef = useRef<ThumbnailPipeline | null>(null);
  const setGridPerf = useSetAtom(gridPerfAtom);
  const setGridFrameTrace = useSetAtom(gridFrameTraceAtom);
  const perfEnabled = useMemo(diagnosticsEnabled, []);
  const pendingTraceEventsRef = useRef<PendingTraceEvent[]>([]);
  const activeTraceRef = useRef<{ startedAt: number; trace: GridFrameTrace } | null>(null);
  const pendingScheduleTraceRef = useRef<PendingScheduleTrace>({
    reasons: [],
    requestedAt: null,
    firedAt: null,
    hadPendingRaf: false,
    staleReset: false,
  });
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

  const queueTraceEvent = useCallback((type: string, payload?: Record<string, unknown>) => {
    const settings = gridTraceRecorder.getSettings();
    if (!settings.enabled) return;
    if (activeTraceRef.current) {
      activeTraceRef.current.trace.events.push(
        createTraceEvent(activeTraceRef.current.startedAt, type, payload, settings),
      );
      return;
    }
    pendingTraceEventsRef.current.push({
      createdAt: performance.now(),
      type,
      payload: settings.includeEventPayloads ? payload : undefined,
    });
    if (pendingTraceEventsRef.current.length > 1000) {
      pendingTraceEventsRef.current.splice(0, pendingTraceEventsRef.current.length - 1000);
    }
  }, []);

  const drainTraceEvents = useCallback((frameStartedAt: number): GridTraceEvent[] => {
    const settings = gridTraceRecorder.getSettings();
    const events = pendingTraceEventsRef.current.splice(0, pendingTraceEventsRef.current.length);
    return events.map((event) => ({
      atMs: Math.max(0, event.createdAt - frameStartedAt),
      type: event.type,
      payload: settings.includeEventPayloads ? event.payload : undefined,
    }));
  }, []);

  useEffect(() => {
    profilerRef.current.setWarnOnDrop(false);
  }, []);

  useEffect(() => gridTraceRecorder.subscribe((snapshot) => {
    setGridFrameTrace(snapshot);
  }), [setGridFrameTrace]);

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
  }, [aspectRatios, renderItems.length, textHeight, viewMode]);

  const scheduleRedraw = useCallback((reason = 'unknown') => {
    const pendingSchedule = pendingScheduleTraceRef.current;
    if (rafRef.current != null) {
      const ageMs = performance.now() - rafScheduledAtRef.current;
      pendingSchedule.hadPendingRaf = true;
      if (ageMs > 100) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
        pendingSchedule.staleReset = true;
        queueTraceEvent('schedule.stale_raf_reset', {
          ageMs,
          reason,
          dirty: { ...dirtyRef.current },
        });
      } else {
        queueTraceEvent('schedule.skipped_existing_raf', {
          ageMs,
          reason,
          dirty: { ...dirtyRef.current },
        });
        return;
      }
    }
    if (!pendingSchedule.reasons.includes(reason)) {
      pendingSchedule.reasons.push(reason);
    }
    pendingSchedule.requestedAt = performance.now();
    rafScheduledCountRef.current += 1;
    queueTraceEvent('schedule.requested', {
      reason,
      dirty: { ...dirtyRef.current },
    });
    rafScheduledAtRef.current = pendingSchedule.requestedAt;
    rafRef.current = requestAnimationFrame(() => {
      pendingSchedule.firedAt = performance.now();
      rafFiredAtRef.current = pendingSchedule.firedAt;
      rafExecutedCountRef.current += 1;
      if (scrollActiveRef.current) rafFramesWhileScrollingRef.current += 1;
      else rafFramesWhileIdleRef.current += 1;
      rafRef.current = null;
      rafScheduledAtRef.current = 0;
      drawRef.current();
    });
  }, [queueTraceEvent]);

  const markDirty = useCallback((lane: DirtyLane, reason = 'dirty') => {
    if (lane === 'base' || lane === 'both') dirtyRef.current.base = true;
    if (lane === 'overlay' || lane === 'both') dirtyRef.current.overlay = true;
    scheduleRedraw(reason);
  }, [scheduleRedraw]);

  useEffect(() => {
    const pipeline = new ThumbnailPipeline(() => {
      if (scrollActiveRef.current) {
        pendingPipelineDirtyRef.current = true;
        queueTraceEvent('pipeline.dirty_deferred', {
          scrollPhase: scrollStateRef.current.phase,
          scrollDirection: scrollStateRef.current.direction,
        });
        return;
      }
      markDirty('base', 'pipeline_dirty');
    });
    pipeline.setTraceListener((event) => {
      const hash = typeof event.payload.hash === 'string' ? event.payload.hash : null;
      if (event.type === 'bitmap_ready' && hash) {
        pendingPromotionHashesRef.current.add(hash);
      } else if (
        hash
        && (event.type === 'bitmap_promoted'
          || event.type === 'stale_result_dropped'
          || event.type === 'evicted'
          || event.type === 'queue_became_stale'
          || event.type === 'inflight_canceled')
      ) {
        pendingPromotionHashesRef.current.delete(hash);
      }
      queueTraceEvent(`pipeline.${event.type}`, event.payload);
    });
    pipelineRef.current = pipeline;
    return () => {
      pipeline.setTraceListener(null);
      pipeline.clear();
      pipelineRef.current = null;
    };
  }, [markDirty, queueTraceEvent]);

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
      queueTraceEvent('draw.aborted_missing_root', {
        hasContainer: !!container,
        hasPipeline: !!pipeline,
      });
      return;
    }

    const frameStartedAt = performance.now();
    const traceEnabled = gridTraceRecorder.shouldCapture(scrollActiveRef.current);
    const shouldProfile = perfEnabled || traceEnabled;
    const traceSettings = gridTraceRecorder.getSettings();
    const pendingSchedule = pendingScheduleTraceRef.current;
    let frameTrace: GridFrameTrace | null = null;
    if (traceEnabled) {
      frameTrace = {
        frameId: gridTraceRecorder.allocateFrameId(),
        startedAt: frameStartedAt,
        endedAt: frameStartedAt,
        durationMs: 0,
        budgetMs: 8.33,
        status: 'ok',
        cause: 'unknown',
        scrollState: {
          active: scrollActiveRef.current,
          phase: scrollStateRef.current.phase,
          direction: scrollStateRef.current.direction,
          velocityPxPerSec: scrollStateRef.current.velocityPxPerSec,
        },
        raf: {
          requestedAt: pendingSchedule.requestedAt,
          firedAt: pendingSchedule.firedAt ?? rafFiredAtRef.current,
          delayMs: pendingSchedule.requestedAt && (pendingSchedule.firedAt ?? rafFiredAtRef.current)
            ? (pendingSchedule.firedAt ?? rafFiredAtRef.current ?? pendingSchedule.requestedAt) - pendingSchedule.requestedAt
            : 0,
          frameGapMs: 0,
          hadPendingRaf: pendingSchedule.hadPendingRaf,
          staleReset: pendingSchedule.staleReset,
          reasons: [...pendingSchedule.reasons],
        },
        visibility: {
          startIdx: 0,
          endIdx: 0,
          visibleIterEnd: 0,
          prefetchCount: 0,
          cancelTop: 0,
          cancelBottom: 0,
        },
        pipeline: {
          queueDepth: 0,
          activeLoads: 0,
          cacheEntries: 0,
          totalBytes: 0,
          visibleThumbs: {
            unique: 0,
            ready: 0,
            loading: 0,
            queued: 0,
            missing: 0,
          },
          ensureVisibleCount: 0,
          ensurePrefetchCount: 0,
          cancelCount: 0,
          evictCount: 0,
          staleWorkCount: 0,
          visibleImpactCount: 0,
        },
        draw: {
          preconditionsMs: 0,
          visibilityMs: 0,
          pipelineMs: 0,
          clearMs: 0,
          imageDrawMs: 0,
          chromeDrawMs: 0,
          totalMs: 0,
        },
        outcome: {
          firstPaint: false,
          activeReveal: false,
          scheduledNextFrame: false,
          dirtyBefore: { ...dirtyRef.current },
          dirtyAfter: { ...dirtyRef.current },
        },
        events: drainTraceEvents(frameStartedAt),
      };
      activeTraceRef.current = { startedAt: frameStartedAt, trace: frameTrace };
      if (frameTrace.raf.firedAt != null) {
        frameTrace.events.push(createTraceEvent(frameStartedAt, 'raf.fired', {
          requestedAt: frameTrace.raf.requestedAt,
          firedAt: frameTrace.raf.firedAt,
          delayMs: frameTrace.raf.delayMs,
        }, traceSettings));
      }
      frameTrace.events.push(createTraceEvent(frameStartedAt, 'draw.started', {
        items: renderItems.length,
        dirty: { ...dirtyRef.current },
      }, traceSettings));
    }

    const finalizeTrace = (
      status: GridTraceStatus,
      eventType: string,
      payload?: Record<string, unknown>,
      cause: GridTraceCause = 'unknown',
    ) => {
      if (!frameTrace) return;
      frameTrace.events.push(createTraceEvent(frameStartedAt, eventType, payload, traceSettings));
      frameTrace.endedAt = performance.now();
      frameTrace.durationMs = frameTrace.endedAt - frameTrace.startedAt;
      frameTrace.draw.totalMs = frameTrace.draw.totalMs || frameTrace.durationMs;
      frameTrace.status = status;
      frameTrace.cause = cause;
      frameTrace.outcome.dirtyAfter = { ...dirtyRef.current };
      gridTraceRecorder.record(frameTrace);
      activeTraceRef.current = null;
    };

    pendingScheduleTraceRef.current = {
      reasons: [],
      requestedAt: null,
      firedAt: null,
      hadPendingRaf: false,
      staleReset: false,
    };

    let { width, height } = containerDimsRef.current;
    if (width <= 0 || height <= 0) {
      const measured = measureContainerSize(container);
      width = measured.width;
      height = measured.height;
      if (width <= 0 || height <= 0) {
        finalizeTrace('aborted', 'draw.aborted_zero_size', {
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
    const scrollPositionChanged = lastPresentedScrollTopRef.current == null
      || Math.abs(scrollTop - lastPresentedScrollTopRef.current) > 0.01;
    if (scrollPositionChanged) {
      dirtyRef.current.base = true;
      queueTraceEvent('scroll.position_changed', {
        previousScrollTop: lastPresentedScrollTopRef.current,
        scrollTop,
      });
    }
    const sizing = ensureCanvasContexts(width, height);
    const baseCtx = baseContextRef.current;
    const overlayCtx = overlayContextRef.current;
    if (!sizing || !baseCtx || !overlayCtx) {
      finalizeTrace('aborted', 'draw.aborted_missing_context', {
        hasSizing: !!sizing,
        hasBaseCtx: !!baseCtx,
        hasOverlayCtx: !!overlayCtx,
        items: renderItems.length,
        width,
        height,
      });
      return;
    }

    if (!dirtyRef.current.base && !dirtyRef.current.overlay) {
      skippedFrameCountRef.current += 1;
      const shouldKeepScrollPumpAlive = interactive && (scrollActiveRef.current || scrollPositionChanged);
      if (shouldKeepScrollPumpAlive) {
        scheduleRedraw('scroll_pump');
      }
      finalizeTrace('skipped', 'draw.skipped_clean', {
        items: renderItems.length,
        width,
        height,
        keepScrollPumpAlive: shouldKeepScrollPumpAlive,
      }, 'idle_noise');
      return;
    }

    const { dpr } = sizing;
    const preconditionsEndAt = performance.now();
    if (layoutRef.current.positions.length !== renderItems.length) {
      const scrollbarWidth = container.offsetWidth - width;
      applyLayout(width, height, scrollbarWidth);
    }
    const { positions } = layoutRef.current;
    const profiler = profilerRef.current;
    if (shouldProfile) profiler.begin();
    if (frameTrace) {
      frameTrace.draw.preconditionsMs = preconditionsEndAt - frameStartedAt;
      frameTrace.events.push(createTraceEvent(frameStartedAt, 'draw.preconditions', {
        width,
        height,
        canvasWidth: baseCanvasRef.current?.width ?? 0,
        canvasHeight: baseCanvasRef.current?.height ?? 0,
        items: renderItems.length,
        layoutCount: positions.length,
      }, traceSettings));
    }

    const visibilityStartedAt = performance.now();
    pipeline.setScrollState(scrollStateRef.current);
    const plan = buildCanvasVisibilityPlan({
      positions,
      scrollTop,
      viewportHeight: height,
      scrollPhase: scrollStateRef.current.phase,
      scrollDirection: scrollStateRef.current.direction,
      queueDepth: pipeline.getStats().queueDepth,
    });
    const visibilityEndedAt = performance.now();
    if (frameTrace) {
      frameTrace.visibility = {
        startIdx: plan.startIdx,
        endIdx: plan.endIdx,
        visibleIterEnd: plan.visibleIterEnd,
        prefetchCount: plan.prefetchIndices.length,
        cancelTop: plan.cancelTop,
        cancelBottom: plan.cancelBottom,
      };
      frameTrace.draw.visibilityMs = visibilityEndedAt - visibilityStartedAt;
      frameTrace.events.push(createTraceEvent(frameStartedAt, 'visibility.planned', {
        ...frameTrace.visibility,
        scrollTop,
        viewportHeight: height,
      }, traceSettings));
    }
    if (shouldProfile) profiler.mark(Phase.visibilityPlan);

    const visibleHashes = new Set<string>();
    let ready = 0;
    let loading = 0;
    let queued = 0;
    let missing = 0;
    let ensureVisibleCount = 0;

    for (let n = 0; n < plan.visibleIterEnd; n += 1) {
      const idx = plan.visibleIndices ? plan.visibleIndices[n] : plan.startIdx + n;
      if (idx >= plan.endIdx || idx >= renderItems.length) break;
      const pos = positions[idx];
      const item = renderItems[idx];
      if (!pos || !item || visibleHashes.has(item.thumbnailHash)) continue;
      visibleHashes.add(item.thumbnailHash);
      ensureVisibleCount += 1;
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

    const pendingPromotionHashes = pendingPromotionHashesRef.current;
    let promotedVisibleCount = 0;
    let promotedPrefetchCount = 0;
    if (pendingPromotionHashes.size > 0) {
      const visibleBudget = scrollActiveRef.current ? SCROLL_VISIBLE_PROMOTION_BUDGET : IDLE_VISIBLE_PROMOTION_BUDGET;
      const prefetchBudget = scrollActiveRef.current ? 0 : IDLE_PREFETCH_PROMOTION_BUDGET;

      for (const hash of Array.from(pendingPromotionHashes)) {
        if (promotedVisibleCount >= visibleBudget) break;
        if (!visibleHashes.has(hash)) continue;
        if (pipeline.promote(hash)) {
          pendingPromotionHashes.delete(hash);
          promotedVisibleCount += 1;
        }
      }

      if (prefetchBudget > 0) {
        for (const hash of Array.from(pendingPromotionHashes)) {
          if (promotedPrefetchCount >= prefetchBudget) break;
          if (visibleHashes.has(hash)) continue;
          if (pipeline.promote(hash)) {
            pendingPromotionHashes.delete(hash);
            promotedPrefetchCount += 1;
          }
        }
      }
    }

    ready = 0;
    loading = 0;
    queued = 0;
    missing = 0;
    for (const hash of visibleHashes) {
      const entry = pipeline.get(hash);
      switch (entry?.state ?? 'idle') {
        case 'shown': ready += 1; break;
        case 'loading':
        case 'ready_pending': loading += 1; break;
        case 'queued': queued += 1; break;
        default: missing += 1; break;
      }
    }

    if (frameTrace) {
      frameTrace.pipeline.ensureVisibleCount = ensureVisibleCount;
    }
    if (shouldProfile) profiler.mark(Phase.hashCollection);

    const pipelineStartedAt = performance.now();
    let ensurePrefetchCount = 0;
    for (const idx of plan.prefetchIndices) {
      const pos = positions[idx];
      const item = renderItems[idx];
      if (!pos || !item) continue;
      ensurePrefetchCount += 1;
      pipeline.ensure(item.thumbnailHash, { y: pos.y + pos.h / 2 });
    }
    pipeline.cancelOutsideWindow(plan.cancelTop, plan.cancelBottom);

    const evictState = evictStateRef.current;
    const evictNow = performance.now();
    let evictCount = 0;
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
        evictCount = batch.evicted.length;
        pipeline.evictHashes(batch.evicted);
      }
    }
    const pipelineEndedAt = performance.now();
    const pipelineStats = pipeline.getStats();
    if (frameTrace) {
      frameTrace.pipeline = {
        queueDepth: pipelineStats.queueDepth,
        activeLoads: pipelineStats.activeLoads,
        cacheEntries: pipelineStats.cacheEntries,
        totalBytes: pipelineStats.totalBytes,
        visibleThumbs: {
          unique: visibleHashes.size,
          ready,
          loading,
          queued,
          missing,
        },
        ensureVisibleCount,
        ensurePrefetchCount,
        cancelCount: 0,
        evictCount,
        staleWorkCount: 0,
        visibleImpactCount: 0,
      };
      frameTrace.draw.pipelineMs = pipelineEndedAt - pipelineStartedAt;
      frameTrace.events.push(createTraceEvent(frameStartedAt, 'pipeline.snapshot', {
        queueDepth: pipelineStats.queueDepth,
        activeLoads: pipelineStats.activeLoads,
        cacheEntries: pipelineStats.cacheEntries,
        totalBytes: pipelineStats.totalBytes,
        ensureVisibleCount,
        ensurePrefetchCount,
        evictCount,
        promotedVisibleCount,
        promotedPrefetchCount,
        pendingPromotions: pendingPromotionHashes.size,
      }, traceSettings));
    }
    if (shouldProfile) profiler.mark(Phase.pipeline);

    visibleTileCountRef.current = plan.visibleIterEnd;
    drawnFrameCountRef.current += 1;
    if (scrollActiveRef.current) scrollFramesRef.current += 1;
    visibleThumbStateCountsRef.current = {
      unique: visibleHashes.size,
      ready,
      loading,
      queued,
      missing,
    };

    let hasActiveReveal = false;
    if (dirtyRef.current.base) {
      const drawNow = suppressTileReveal ? Number.MAX_SAFE_INTEGER : performance.now();
      const clearStartedAt = performance.now();
      baseCtx.setTransform(1, 0, 0, 1, 0, 0);
      baseCtx.fillStyle = backgroundColorRef.current;
      baseCtx.fillRect(0, 0, baseCanvasRef.current?.width ?? 0, baseCanvasRef.current?.height ?? 0);
      baseCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const clearEndedAt = performance.now();
      if (frameTrace) frameTrace.draw.clearMs = clearEndedAt - clearStartedAt;
      if (shouldProfile) profiler.mark(Phase.clear);

      const imageDrawStartedAt = performance.now();
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
      const imageDrawEndedAt = performance.now();
      if (frameTrace) {
        frameTrace.draw.imageDrawMs = imageDrawEndedAt - imageDrawStartedAt;
        frameTrace.draw.chromeDrawMs = 0;
      }
      if (shouldProfile) {
        profiler.mark(Phase.imageDraw);
        profiler.mark(Phase.chromeDraw);
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
      if (frameTrace) {
        frameTrace.outcome.firstPaint = true;
      }
      onFirstPaintRef.current?.();
    }

    const commit = shouldProfile ? profiler.end({
        visibleTiles: visibleTileCountRef.current,
        expectContinuousFrames: hasActiveReveal,
      }) : null;

    if (hasActiveReveal) {
      dirtyRef.current.base = true;
      scheduleRedraw('active_reveal');
    } else if (pendingPromotionHashesRef.current.size > 0) {
      dirtyRef.current.base = true;
      if (interactive && scrollActiveRef.current) {
        scheduleRedraw('promotion_pump');
      } else {
        scheduleRedraw('promotion_flush');
      }
    } else if (interactive && (scrollActiveRef.current || scrollPositionChanged)) {
      scheduleRedraw('scroll_pump');
    }

    if (frameTrace) {
      const staleEvents = frameTrace.events.filter((event) => (
        event.type === 'pipeline.queue_became_stale'
        || event.type === 'pipeline.inflight_canceled'
        || event.type === 'pipeline.late_worker_response'
        || event.type === 'pipeline.stale_result_dropped'
        || event.type === 'pipeline.evicted'
        || event.type === 'pipeline.dirty_deferred'
      ));
      const visibleImpactCount = staleEvents.filter((event) => {
        const hash = typeof event.payload?.hash === 'string' ? event.payload.hash : null;
        if (!hash) return false;
        return visibleHashes.has(hash);
      }).length;

      frameTrace.endedAt = performance.now();
      frameTrace.durationMs = frameTrace.endedAt - frameTrace.startedAt;
      frameTrace.draw.totalMs = commit?.totalMs ?? frameTrace.durationMs;
      frameTrace.raf.frameGapMs = commit?.frameGapMs ?? 0;
      frameTrace.outcome.activeReveal = hasActiveReveal;
      frameTrace.outcome.scheduledNextFrame = hasActiveReveal;
      frameTrace.outcome.dirtyAfter = { ...dirtyRef.current };
      frameTrace.events.push(createTraceEvent(frameStartedAt, 'draw.completed', {
        visibleIterEnd: plan.visibleIterEnd,
        ready,
        loading,
        queued,
        missing,
        hasActiveReveal,
      }, traceSettings));
      frameTrace.pipeline.cancelCount = frameTrace.events.filter((event) => (
        event.type === 'pipeline.queue_became_stale' || event.type === 'pipeline.inflight_canceled'
      )).length;
      frameTrace.pipeline.evictCount = Math.max(
        frameTrace.pipeline.evictCount,
        frameTrace.events.filter((event) => event.type === 'pipeline.evicted').length,
      );
      frameTrace.pipeline.staleWorkCount = staleEvents.length;
      frameTrace.pipeline.visibleImpactCount = visibleImpactCount;
      frameTrace.status = classifyTraceStatus(frameTrace.raf.frameGapMs || frameTrace.raf.delayMs, frameTrace.durationMs);
      frameTrace.cause = inferTraceCause(frameTrace);
      gridTraceRecorder.record(frameTrace);
      activeTraceRef.current = null;
    }
    lastPresentedScrollTopRef.current = scrollTop;
    rafFiredAtRef.current = null;
  }, [
    applyLayout,
    drainTraceEvents,
    ensureCanvasContexts,
    frozenScrollTop,
    interactive,
    items.length,
    perfEnabled,
    queueTraceEvent,
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
      const velocityPxPerMs = velocityPxPerSec / 1000;

      lastScrollTopRef.current = scrollTop;
      lastScrollEventAtRef.current = now;
      scrollStateRef.current = {
        phase: classifyCanvasScrollPhase(velocityPxPerSec),
        direction: resolveCanvasScrollDirection(delta),
        velocityPxPerSec,
      };
      scrollEventCountRef.current += 1;
      scrollVelocitySamplesRef.current.sum += velocityPxPerMs;
      scrollVelocitySamplesRef.current.count += 1;
      scrollVelocitySamplesRef.current.max = Math.max(scrollVelocitySamplesRef.current.max, velocityPxPerMs);
      scrollActiveRef.current = true;
      queueTraceEvent('scroll.event', {
        scrollTop,
        delta,
        elapsedMs: elapsed,
        velocityPxPerSec,
        phase: scrollStateRef.current.phase,
        direction: scrollStateRef.current.direction,
      });
      onScrollTopChangeRef.current?.(scrollTop);
      markDirty('base', 'scroll');

      if (scrollIdleTimerRef.current != null) window.clearTimeout(scrollIdleTimerRef.current);
      scrollIdleTimerRef.current = window.setTimeout(() => {
        scrollIdleTimerRef.current = null;
        scrollActiveRef.current = false;
        scrollStateRef.current = createIdleCanvasScrollState();
        queueTraceEvent('scroll.idle', {
          scrollTop: container.scrollTop,
        });
        if (pendingPipelineDirtyRef.current) {
          pendingPipelineDirtyRef.current = false;
          markDirty('base', 'pipeline_flush_after_scroll');
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
      markDirty('base', 'frozen_scroll');
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
    queueTraceEvent('items.updated', {
      items: items.length,
      renderItems: renderItems.length,
      viewMode,
      targetSize,
      showName,
      showExtension,
    });
    recomputeLayout();
    markDirty('both', 'items_updated');
  }, [items, markDirty, queueTraceEvent, recomputeLayout, renderItems.length, showExtension, showName, targetSize, viewMode]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

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

    container.addEventListener('click', handleClick);
    return () => container.removeEventListener('click', handleClick);
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
        scrollFrames: scrollFramesRef.current,
        avgScrollVelocityPxPerMs: scrollVelocitySamplesRef.current.count > 0
          ? scrollVelocitySamplesRef.current.sum / scrollVelocitySamplesRef.current.count
          : 0,
        maxScrollVelocityPxPerMs: scrollVelocitySamplesRef.current.max,
        rafFramesWhileIdle: rafFramesWhileIdleRef.current,
        rafFramesWhileScrolling: rafFramesWhileScrollingRef.current,
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
    <div className={styles.root}>
      <div
        className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}
        ref={containerRef}
      >
        <div className={styles.canvasWrap} ref={wrapRef} />
      </div>
      <div className={styles.canvasViewport} ref={viewportRef}>
        <canvas ref={baseCanvasRef} className={styles.baseCanvas} />
        <canvas ref={overlayCanvasRef} className={styles.overlayCanvas} />
        {perfEnabled && interactive && profilerText && (
          <pre className={styles.profilerOverlay}>{profilerText}</pre>
        )}
      </div>
    </div>
  );
}
