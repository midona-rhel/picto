import { useCallback, type Dispatch, type SetStateAction } from 'react';
import { buildCanvasVisibilityPlan } from '../layout/canvasVisibilityPlan';
import { drawCanvasBaseLayer } from './canvasGridDrawHelpers';
import type { GridViewMode } from '../runtime';
import type { MasonryImageItem } from '../shared';
import type { LayoutItem } from '../layoutMath';
import type { GridDebugStats } from './canvasGridDebug';
import type { ThumbnailPipeline } from '../../../shared/lib/canvas/thumbnailPipeline';

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
  viewModeRef: { current: GridViewMode };
  layoutRef: { current: { positions: LayoutItem[] } };
  bucketIndexRef: { current: Map<number, number[]> | null };
  imagesRef: { current: MasonryImageItem[] };
  lastVisibleRef: { current: LastVisibleState | null };
  textHeightRef: { current: number };
  showTileNameRef: { current: boolean };
  showResolutionRef: { current: boolean };
  showExtensionRef: { current: boolean };
  showExtensionLabelRef: { current: boolean };
  renamingHashRef: { current: string | null };
  videoScrubIdxRef: { current: number | null };
  thumbnailFitMode: 'cover' | 'contain';
  perfRef: { current: {
    frames: number;
    drawMsTotal: number;
    visMsTotal: number;
    slowFrames: number;
    sampleStart: number;
    lastFrameAt: number;
    baseFrames: number;
    overlayFrames: number;
  } };
  gridDebugEnabled: boolean;
  gridDebugSampleMs: number;
  setDebugStats: Dispatch<SetStateAction<GridDebugStats | null>>;
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
    perfRef,
    gridDebugEnabled,
    gridDebugSampleMs,
    setDebugStats,
    markDirty,
  } = args;

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
      ctxRef.current = canvas.getContext('2d', { alpha: true });
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
    atlas.setScrolling(isScrollingRef.current);

    const t0 = import.meta.env.DEV ? performance.now() : 0;

    const positions = layoutRef.current.positions;
    const imgs = imagesRef.current;
    const scrollTop = scrollTopRef.current;
    const vh = viewportHeightRef.current;
    const isScrolling = isScrollingRef.current;

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
      isScrolling,
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

    const tVis = import.meta.env.DEV ? performance.now() : 0;
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

    let prefetched = 0;
    for (let n = 0; n < prefetchIndices.length; n++) {
      const i = prefetchIndices[n];
      const pos = positions[i];
      const image = imgs[i];
      if (!pos || !image) continue;
      atlas.ensure(image.hash, { y: pos.y + pos.h / 2 });
      prefetched++;
    }

    atlas.cancelOutsideWindow(cancelTop, cancelBottom);

    const tEnd = performance.now();
    if (gridDebugEnabled) {
      const perf = perfRef.current;
      const sampleElapsed = Math.max(1, tEnd - perf.sampleStart);
      perf.frames += 1;
      perf.baseFrames += 1;
      perf.drawMsTotal += tEnd - t0;
      perf.visMsTotal += tVis - t0;
      if (tEnd - t0 > 16.7) perf.slowFrames += 1;
      if (sampleElapsed >= gridDebugSampleMs) {
        const atlasStats = atlas.getStats();
        setDebugStats({
          fps: (perf.frames * 1000) / sampleElapsed,
          drawMs: perf.drawMsTotal / perf.frames,
          visMs: perf.visMsTotal / perf.frames,
          visibleTiles: visibleIndices ? visibleIndices.length : Math.max(0, endIdx - startIdx),
          prefetchedTiles: prefetched,
          queueDepth: atlasStats.queueDepth,
          activeLoads: atlasStats.activeLoads,
          pendingThumbs: atlasStats.pendingThumbs,
          cacheSize: atlasStats.cacheSize,
          slowFrames: perf.slowFrames,
          diskSpeed: atlasStats.diskSpeed,
          baseRedraws: perf.baseFrames,
          overlayRedraws: perf.overlayFrames,
        });
        perf.frames = 0;
        perf.baseFrames = 0;
        perf.overlayFrames = 0;
        perf.drawMsTotal = 0;
        perf.visMsTotal = 0;
        perf.slowFrames = 0;
        perf.sampleStart = tEnd;
      }
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
    gridDebugEnabled,
    gridDebugSampleMs,
    imagesRef,
    isScrollingRef,
    lastVisibleRef,
    layoutRef,
    bucketIndexRef,
    markDirty,
    perfRef,
    renamingHashRef,
    scrollTopRef,
    setDebugStats,
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
