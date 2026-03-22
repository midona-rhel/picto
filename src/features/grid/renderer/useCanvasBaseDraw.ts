import { useCallback, useEffect } from 'react';
import { buildCanvasVisibilityPlan } from '../layout/canvasVisibilityPlan';
import { drawCanvasBaseLayer } from './canvasGridDrawHelpers';
import type { GridViewMode } from '../runtime';
import type { MasonryItem } from '../shared';
import type { LayoutItem } from '../layoutMath';
import type { ThumbnailPipeline } from '../../../shared/lib/canvas/thumbnailPipeline';
import type { CanvasScrollDirection, CanvasScrollPhase } from '../../../shared/lib/canvas/scrollState';
import { useNavigationImageAdjustmentsStore } from '../../../state/navigationImageAdjustmentsStore';

/** Persistent state for throttled bitmap eviction (survives across frames). */
const evictState = {
  lastRun: 0,
  keepHashes: new Set<string>(),
  cursor: 0,
};

interface ThemeState {
  primaryColor: string;
  textPrimary: string;
  textTertiary: string;
  placeholderBg: string;
  borderRadius: number;
  innerBorder: string;
}

interface LastVisibleState {
  startIdx: number;
  endIdx: number;
  visibleIndices: number[] | null;
  visibleIterEnd: number;
  scrollTop: number;
  cssH: number;
  th: number;
  br: number;
}

function ensureCanvasSize(canvas: HTMLCanvasElement, dpr: number): [number, number] {
  const cssW = canvas.clientWidth;
  const cssH = canvas.clientHeight;
  const bufW = Math.round(cssW * dpr);
  const bufH = Math.round(cssH * dpr);
  if (canvas.width !== bufW || canvas.height !== bufH) {
    canvas.width = bufW;
    canvas.height = bufH;
  }
  return [cssW, cssH];
}

export function useCanvasBaseDraw(args: {
  frozenRef: { current: boolean };
  canvasRef: { current: HTMLCanvasElement | null };
  ctxRef: { current: CanvasRenderingContext2D | null };
  themeRef: { current: ThemeState | null };
  atlasRef: { current: ThumbnailPipeline | null };
  getScrollMetrics: () => { localScrollTop: number; canvasTopInScroll: number; viewportHeight: number };
  viewportHeightRef: { current: number };
  scrollTopRef: { current: number };
  isScrollingRef: { current: boolean };
  scrollPhaseRef: { current: CanvasScrollPhase };
  scrollDirectionRef: { current: CanvasScrollDirection };
  scrollVelocityRef: { current: number };
  viewModeRef: { current: GridViewMode };
  layoutRef: { current: { positions: LayoutItem[] } };
  bucketIndexRef: { current: Map<number, number[]> | null };
  imagesRef: { current: MasonryItem[] };
  lastVisibleRef: { current: LastVisibleState | null };
  textHeightRef: { current: number };
  showTileNameRef: { current: boolean };
  showResolutionRef: { current: boolean };
  showExtensionRef: { current: boolean };
  showExtensionLabelRef: { current: boolean };
  renamingHashRef: { current: string | null };
  videoScrubIdxRef: { current: number | null };
  thumbnailFitMode: 'cover' | 'contain';
  markDirty: (lanes: 'base' | 'overlay' | 'both') => void;
}) {
  const {
    frozenRef,
    canvasRef,
    ctxRef,
    themeRef,
    atlasRef,
    getScrollMetrics,
    viewportHeightRef,
    scrollTopRef,
    isScrollingRef,
    scrollPhaseRef,
    scrollDirectionRef,
    scrollVelocityRef,
    viewModeRef,
    layoutRef,
    bucketIndexRef,
    imagesRef,
    lastVisibleRef,
    textHeightRef,
    showTileNameRef,
    showResolutionRef,
    showExtensionRef,
    showExtensionLabelRef,
    renamingHashRef,
    videoScrubIdxRef,
    thumbnailFitMode,
    markDirty,
  } = args;

  useEffect(() => {
    return useNavigationImageAdjustmentsStore.subscribe(
      (state, prevState) => {
        if (
          state.byHash !== prevState.byHash
          || state.grayscaleEnabled !== prevState.grayscaleEnabled
        ) {
          markDirty('base');
        }
      },
    );
  }, [markDirty]);

  return useCallback(() => {
    if (frozenRef.current) return;

    const canvas = canvasRef.current;
    if (!canvas) return;

    const metrics = getScrollMetrics();
    if (metrics.viewportHeight > 0) {
      viewportHeightRef.current = metrics.viewportHeight;
    }
    scrollTopRef.current = metrics.localScrollTop;

    if (!ctxRef.current || ctxRef.current.canvas !== canvas) {
      ctxRef.current = canvas.getContext('2d', { alpha: true, desynchronized: true });
    }
    const ctx = ctxRef.current;
    if (!ctx) return;

    if (!themeRef.current) {
      const s = getComputedStyle(document.documentElement);
      themeRef.current = {
        primaryColor: s.getPropertyValue('--color-primary').trim() || '#3297FF',
        textPrimary: s.getPropertyValue('--color-text-primary').trim() || 'rgba(255,255,255,0.92)',
        textTertiary: s.getPropertyValue('--color-text-tertiary').trim() || 'rgba(255,255,255,0.36)',
        placeholderBg: s.getPropertyValue('--tile-placeholder-bg').trim() || 'rgba(255,255,255,0.04)',
        borderRadius: parseInt(s.getPropertyValue('--tile-border-radius').trim(), 10) || 4,
        innerBorder: s.getPropertyValue('--tile-inner-border').trim() || 'rgba(255,255,255,0.05)',
      };
    }
    const theme = themeRef.current;

    const atlas = atlasRef.current;
    if (!atlas) return;
    atlas.setScrollState({
      phase: scrollPhaseRef.current,
      direction: scrollDirectionRef.current,
      velocityPxPerSec: scrollVelocityRef.current,
    });

    const positions = layoutRef.current.positions;
    const imgs = imagesRef.current;
    const scrollTop = scrollTopRef.current;
    const vh = viewportHeightRef.current;
    const scrollPhase = scrollPhaseRef.current;
    const scrollDirection = scrollDirectionRef.current;

    const dpr = window.devicePixelRatio || 1;
    const [cssW, cssH] = ensureCanvasSize(canvas, dpr);

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);

    if (positions.length === 0) {
      lastVisibleRef.current = null;
      return;
    }

    const visibilityPlan = buildCanvasVisibilityPlan({
      positions,
      scrollTop,
      viewportHeight: vh,
      scrollPhase,
      scrollDirection,
      queueDepth: atlas.getStats().queueDepth,
      bucketIndex: bucketIndexRef.current,
    });
    const {
      startIdx,
      endIdx,
      visibleIndices,
      visibleIterEnd,
      prefetchIndices,
      cancelTop,
      cancelBottom,
    } = visibilityPlan;

    const br = theme.borderRadius;
    const th = textHeightRef.current;
    lastVisibleRef.current = { startIdx, endIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br };

    const now = performance.now();
    const hasActiveReveal = drawCanvasBaseLayer({
      ctx,
      positions,
      imgs,
      atlasGet: (hash) => atlas.get(hash),
      atlasEnsure: (hash, y) => atlas.ensure(hash, y),
      now,
      theme,
      visible: { startIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br },
      thumbnailFitMode,
      viewMode: viewModeRef.current,
      renamingHash: renamingHashRef.current,
      showTileName: showTileNameRef.current,
      showResolution: showResolutionRef.current,
      showExtension: showExtensionRef.current,
      showExtensionLabel: showExtensionLabelRef.current,
      videoScrubIdx: videoScrubIdxRef.current,
    });

    for (let n = 0; n < prefetchIndices.length; n++) {
      const i = prefetchIndices[n];
      const pos = positions[i];
      const image = imgs[i];
      if (!pos || !image) continue;
      atlas.ensure(image.thumbnail_hash || image.hash, { y: pos.y + pos.h / 2 });
    }

    atlas.cancelOutsideWindow(cancelTop, cancelBottom);

    // Throttled eviction: process up to 5 cache entries every ~33ms.
    // Keep zone = viewport ± 1 full viewport height.
    const evictNow = performance.now();
    if (evictNow - evictState.lastRun >= 33) {
      evictState.lastRun = evictNow;
      const keepTop = scrollTop - cssH;
      const keepBottom = scrollTop + cssH + cssH;
      // Rebuild keep set from visible + nearby tiles.
      // Use the visible indices + scan a band around them.
      evictState.keepHashes.clear();
      // Add all currently visible tiles
      for (let n = 0; n < visibleIterEnd; n++) {
        const idx = visibleIndices ? visibleIndices[n] : startIdx + n;
        const img = imgs[idx];
        if (img) evictState.keepHashes.add(img.thumbnail_hash || img.hash);
      }
      // Add prefetch tiles
      for (const idx of prefetchIndices) {
        const img = imgs[idx];
        if (img) evictState.keepHashes.add(img.thumbnail_hash || img.hash);
      }
      // Add tiles in the keep zone (scan ±30 around visible range)
      const scanStart = Math.max(0, startIdx - 30);
      const scanEnd = Math.min(positions.length, endIdx + 30);
      for (let idx = scanStart; idx < scanEnd; idx++) {
        const p = positions[idx];
        if (!p) continue;
        if (p.y + p.h >= keepTop && p.y <= keepBottom) {
          const img = imgs[idx];
          if (img) evictState.keepHashes.add(img.thumbnail_hash || img.hash);
        }
      }
      // Check up to 5 cache entries per tick
      const batch = atlas.getEvictCandidatesBatch(evictState.keepHashes, 5, evictState.cursor);
      if (batch.evicted.length > 0) atlas.evictHashes(batch.evicted);
      evictState.cursor = batch.nextCursor;
    }

    if (hasActiveReveal) {
      markDirty('base');
    }
  }, [
    atlasRef,
    canvasRef,
    ctxRef,
    frozenRef,
    getScrollMetrics,
    imagesRef,
    isScrollingRef,
    scrollDirectionRef,
    scrollPhaseRef,
    scrollVelocityRef,
    lastVisibleRef,
    layoutRef,
    bucketIndexRef,
    markDirty,
    renamingHashRef,
    scrollTopRef,
    showExtensionLabelRef,
    showExtensionRef,
    showResolutionRef,
    showTileNameRef,
    textHeightRef,
    themeRef,
    thumbnailFitMode,
    videoScrubIdxRef,
    viewModeRef,
    viewportHeightRef,
  ]);
}
