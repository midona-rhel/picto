import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';
import { Application } from './pixiRuntime';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { adaptGridItem, type CanvasRenderItem } from '../canvas/renderItemAdapter';
import type { CanvasGridProps } from '../canvas/DomGridFallback';
import { mediaThumbnailUrl } from '../canvas/mediaUrl';
import { safeAspectRatio } from '../layout/layoutMath';
import { GridLayoutIndex } from './GridLayoutIndex';
import { detectScrollPlatformProfile, GridScrollController } from './GridScrollController';
import {
  GridSceneRenderer,
  type GridScenePerfSample,
  type GridSceneSnapshot,
  type SceneTileSnapshot,
} from './GridSceneRenderer';
import { GridTextureStore } from './GridTextureStore';
import styles from '../canvas/CanvasGrid.module.css';

const GAP = 12;
const TEXT_NAME_ROW_H = 20;
const LOAD_MORE_THRESHOLD_PX = 400;
const MIN_OVERSCAN_PX = 320;
const PRELOAD_START_DELAY_MS = 30;
const MODULO_STRESS_TEST_ITEM_COUNT = 999_999;
const TEXTURE_BUDGET_BYTES = 1024 * 1024 * 1024;
const SCROLL_REACT_DEBOUNCE_MS = 100;

interface GridSceneHostProps extends CanvasGridProps {
  onFallback?: () => void;
}

function measureContainerSize(container: HTMLDivElement): { width: number; height: number } {
  const rect = container.getBoundingClientRect();
  const width = container.clientWidth || Math.round(rect.width);
  const height = container.clientHeight || Math.round(rect.height);
  return { width, height };
}

function distance(aX: number, aY: number, bX: number, bY: number): number {
  const dx = aX - bX;
  const dy = aY - bY;
  return Math.sqrt(dx * dx + dy * dy);
}

interface SnapshotBuildParams {
  layoutIndex: GridLayoutIndex;
  containerWidth: number;
  containerHeight: number;
  getItemAtIndex: (index: number) => CanonicalEntityGridItem | null;
  getRenderItemAtIndex: (index: number) => CanvasRenderItem | null;
  showName: boolean;
  showExtension: boolean;
  viewMode: 'waterfall' | 'grid' | 'justified';
  suppressTileReveal: boolean;
  stressMode: boolean;
  platform: ReturnType<typeof detectScrollPlatformProfile>;
}

function buildTiles(
  scrollTop: number,
  params: SnapshotBuildParams,
): SceneTileSnapshot[] {
  const {
    layoutIndex, containerHeight, getItemAtIndex, getRenderItemAtIndex,
    showName, showExtension, viewMode, suppressTileReveal, stressMode,
  } = params;
  const overscanPx = Math.max(Math.floor(containerHeight * 0.5), MIN_OVERSCAN_PX);
  const renderRange = layoutIndex.getRange(scrollTop, containerHeight, overscanPx);
  const visibleRange = layoutIndex.getRange(scrollTop, containerHeight, 0);
  const activationOverscanPx = Math.floor(containerHeight * 0.5);
  const activationRange = layoutIndex.getRange(scrollTop, containerHeight, activationOverscanPx);

  const visibleSet = new Set(visibleRange.visibleIndices);
  const activationSet = new Set(activationRange.renderedIndices);
  const tiles: SceneTileSnapshot[] = [];

  for (const index of renderRange.renderedIndices) {
    const item = getItemAtIndex(index);
    const position = layoutIndex.getItem(index);
    const renderItem = getRenderItemAtIndex(index);
    if (!item || !position || !renderItem) continue;
    tiles.push({
      index,
      itemHash: item.entity_hash,
      thumbnailHash: item.thumbnail_hash,
      renderItem,
      position: {
        ...position,
        y: position.y - scrollTop,
      },
      isVisible: visibleSet.has(index),
      isInActivationRange: activationSet.has(index),
      showName,
      showExtension,
      viewMode,
      suppressTileReveal,
      hovered: false,
      showStressIndex: stressMode,
    });
  }
  return tiles;
}

function buildSnapshot(
  scrollTop: number,
  params: SnapshotBuildParams,
  scrollController: GridScrollController,
): GridSceneSnapshot {
  const tiles = buildTiles(scrollTop, params);
  return {
    viewportWidth: params.containerWidth,
    viewportHeight: params.containerHeight,
    platform: params.platform,
    scrollbar: scrollController.getScrollbarState(params.containerWidth),
    tiles,
  };
}

export function GridSceneHost({
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
  onFallback,
}: GridSceneHostProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const appRef = useRef<Application | null>(null);
  const sceneRendererRef = useRef<GridSceneRenderer | null>(null);
  const textureStoreRef = useRef<GridTextureStore | null>(null);
  const scrollControllerRef = useRef<GridScrollController | null>(null);
  const pointerDownRef = useRef<{ pointerId: number; x: number; y: number; index: number | null } | null>(null);
  const renderItemCacheRef = useRef(new Map<string, CanvasRenderItem>());
  const firstPaintNotifiedRef = useRef(false);
  const stressSourceLengthRef = useRef(0);
  const loadMorePendingRef = useRef(false);
  const preloadTimerRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const activationSinceByHashRef = useRef(new Map<string, number>());
  const committedScrollTopRef = useRef(frozenScrollTop);
  const scrollDebounceRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);
  const latestTilesRef = useRef<SceneTileSnapshot[]>([]);
  const buildParamsRef = useRef<SnapshotBuildParams | null>(null);
  const onScrollTopChangeRef = useRef(onScrollTopChange);
  onScrollTopChangeRef.current = onScrollTopChange;
  const onLoadMoreRef = useRef(onLoadMore);
  onLoadMoreRef.current = onLoadMore;

  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [scrollTopForReact, setScrollTopForReact] = useState(frozenScrollTop);
  const [fps, setFps] = useState(0);
  const [textureStoreReadyVersion, setTextureStoreReadyVersion] = useState(0);
  const [sceneRendererReadyVersion, setSceneRendererReadyVersion] = useState(0);
  const platform = useMemo(() => detectScrollPlatformProfile(), []);

  const stressSourceLength = stressSourceLengthRef.current > 0
    ? stressSourceLengthRef.current
    : items.length;
  const effectiveItemCount = stressSourceLength > 0 ? MODULO_STRESS_TEST_ITEM_COUNT : 0;
  const stressMode = effectiveItemCount > stressSourceLength;
  const getItemAtIndex = useCallback((index: number): CanonicalEntityGridItem | null => {
    if (stressSourceLength <= 0) return null;
    return items[index % stressSourceLength] ?? null;
  }, [items, stressSourceLength]);

  const getRenderItemAtIndex = useCallback((index: number): CanvasRenderItem | null => {
    const item = getItemAtIndex(index);
    if (!item) return null;
    const cached = renderItemCacheRef.current.get(item.entity_hash);
    if (cached) return cached;
    const adapted = adaptGridItem(item);
    renderItemCacheRef.current.set(item.entity_hash, adapted);
    return adapted;
  }, [getItemAtIndex]);

  useEffect(() => {
    renderItemCacheRef.current.clear();
  }, [items]);

  useEffect(() => {
    activationSinceByHashRef.current.clear();
  }, [items[0]?.entity_hash]);

  useEffect(() => {
    if (items.length === 0) {
      stressSourceLengthRef.current = 0;
      loadMorePendingRef.current = false;
      return;
    }
    stressSourceLengthRef.current = items.length;
    loadMorePendingRef.current = false;
  }, [items[0]?.entity_hash]);

  useEffect(() => {
    loadMorePendingRef.current = false;
  }, [items.length]);

  const handleRendererSample = useCallback((sample: GridScenePerfSample) => {
    setFps(sample.tickerFps);
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const updateSize = () => {
      const { width, height } = measureContainerSize(host);
      setContainerSize({ width, height });
    };

    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  // Imperative scroll push — bypasses React entirely
  const pushSnapshotImperative = useCallback((nextScrollTop: number) => {
    const renderer = sceneRendererRef.current;
    const controller = scrollControllerRef.current;
    const params = buildParamsRef.current;
    if (!renderer || !controller || !params) return;

    const snapshot = buildSnapshot(nextScrollTop, params, controller);
    latestTilesRef.current = snapshot.tiles;
    renderer.setSnapshot(snapshot);
  }, []);

  useEffect(() => {
    const controller = new GridScrollController({
      platform,
      onChange: (nextScrollTop) => {
        if (Math.abs(nextScrollTop - committedScrollTopRef.current) < 0.25) return;
        committedScrollTopRef.current = nextScrollTop;

        // Imperative fast path — push snapshot directly to renderer
        pushSnapshotImperative(nextScrollTop);
        onScrollTopChangeRef.current?.(nextScrollTop);

        // Debounced React state update for onLoadMore
        if (scrollDebounceRef.current != null) {
          globalThis.clearTimeout(scrollDebounceRef.current);
        }
        scrollDebounceRef.current = globalThis.setTimeout(() => {
          scrollDebounceRef.current = null;
          setScrollTopForReact(nextScrollTop);
        }, SCROLL_REACT_DEBOUNCE_MS);
      },
    });
    controller.setInteractive(interactive);
    scrollControllerRef.current = controller;
    return () => {
      if (preloadTimerRef.current != null) {
        globalThis.clearTimeout(preloadTimerRef.current);
        preloadTimerRef.current = null;
      }
      if (scrollDebounceRef.current != null) {
        globalThis.clearTimeout(scrollDebounceRef.current);
        scrollDebounceRef.current = null;
      }
      scrollControllerRef.current = null;
    };
  }, [interactive, platform, pushSnapshotImperative]);

  useEffect(() => {
    const store = new GridTextureStore({
      byteBudget: TEXTURE_BUDGET_BYTES,
      onChange: () => {
        sceneRendererRef.current?.markTexturesDirty();
      },
    });
    textureStoreRef.current = store;
    sceneRendererRef.current?.setTextureStore(store);
    setTextureStoreReadyVersion((value) => value + 1);
    return () => {
      sceneRendererRef.current?.setTextureStore(null);
      store.destroy();
      textureStoreRef.current = null;
      setTextureStoreReadyVersion((value) => value + 1);
    };
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    let cancelled = false;
    let cleanupContextLost: (() => void) | null = null;

    const mount = async () => {
      try {
        const app = new Application();
        await app.init({
          preference: 'webgl',
          resizeTo: host,
          antialias: true,
          autoDensity: true,
          resolution: Math.max(1, globalThis.window?.devicePixelRatio ?? 1),
          backgroundAlpha: 0,
        });

        if (cancelled) {
          app.destroy(true, { children: true });
          return;
        }

        const sceneRenderer = new GridSceneRenderer(app, {
          onSample: handleRendererSample,
        });
        const canvas = app.canvas as HTMLCanvasElement;
        canvas.style.width = '100%';
        canvas.style.height = '100%';
        canvas.style.display = 'block';
        const handleContextLost = (event: Event) => {
          event.preventDefault();
          onFallback?.();
        };

        canvas.addEventListener('webglcontextlost', handleContextLost, { passive: false });
        cleanupContextLost = () => canvas.removeEventListener('webglcontextlost', handleContextLost);
        host.appendChild(canvas);
        appRef.current = app;
        sceneRendererRef.current = sceneRenderer;
        if (textureStoreRef.current) {
          sceneRenderer.setTextureStore(textureStoreRef.current);
        }
        setSceneRendererReadyVersion((value) => value + 1);
      } catch {
        onFallback?.();
      }
    };

    void mount();

    return () => {
      cancelled = true;
      cleanupContextLost?.();
      sceneRendererRef.current?.destroy();
      sceneRendererRef.current = null;
      setSceneRendererReadyVersion((value) => value + 1);
      if (appRef.current) {
        const canvas = appRef.current.canvas as HTMLCanvasElement;
        if (canvas.parentElement === host) host.removeChild(canvas);
        appRef.current.destroy(true, { children: true });
        appRef.current = null;
      }
    };
  }, [handleRendererSample, onFallback]);

  const textHeight = showName ? TEXT_NAME_ROW_H : 0;
  const layoutIndex = useMemo(() => {
    const next = new GridLayoutIndex();
    next.invalidate({
      itemCount: effectiveItemCount,
      getAspectRatio: (index) => {
        const item = getItemAtIndex(index);
        return safeAspectRatio(
          item?.pixel_width && item.pixel_height ? item.pixel_width / item.pixel_height : 1.5,
        );
      },
      containerWidth: containerSize.width,
      targetSize,
      gap: GAP,
      viewMode,
      textHeight,
      scrollbarWidth: 0,
    });
    return next;
  }, [containerSize.width, effectiveItemCount, getItemAtIndex, targetSize, textHeight, viewMode]);

  // Keep buildParamsRef up to date so the imperative scroll path has fresh values
  const buildParams: SnapshotBuildParams = useMemo(() => ({
    layoutIndex,
    containerWidth: containerSize.width,
    containerHeight: containerSize.height,
    getItemAtIndex,
    getRenderItemAtIndex,
    showName,
    showExtension,
    viewMode,
    suppressTileReveal,
    stressMode,
    platform,
  }), [
    containerSize.height, containerSize.width, getItemAtIndex, getRenderItemAtIndex,
    layoutIndex, platform, showExtension, showName, stressMode, suppressTileReveal, viewMode,
  ]);
  buildParamsRef.current = buildParams;

  useEffect(() => {
    firstPaintNotifiedRef.current = false;
  }, [items[0]?.entity_hash]);

  useEffect(() => {
    const controller = scrollControllerRef.current;
    if (!controller) return;
    controller.setInteractive(interactive);
    controller.setMetrics(layoutIndex.getTotalHeight(), containerSize.height);
    if (!interactive) {
      committedScrollTopRef.current = frozenScrollTop;
      controller.setScrollOffset(frozenScrollTop);
    }
  }, [containerSize.height, frozenScrollTop, interactive, layoutIndex]);

  // Debounced scroll position drives texture preloading (not the scroll hot path)
  const preloadScrollTop = interactive ? scrollTopForReact : frozenScrollTop;
  const activationOverscanPx = Math.floor(containerSize.height * 0.5);
  const activationRange = useMemo(
    () => layoutIndex.getRange(preloadScrollTop, containerSize.height, activationOverscanPx),
    [activationOverscanPx, preloadScrollTop, containerSize.height, layoutIndex],
  );

  useEffect(() => {
    const store = textureStoreRef.current;
    if (!store) return;
    if (preloadTimerRef.current != null) {
      globalThis.clearTimeout(preloadTimerRef.current);
      preloadTimerRef.current = null;
    }
    const activationSinceByHash = activationSinceByHashRef.current;
    const retained = new Set<string>();
    const now = performance.now();

    for (const index of activationRange.renderedIndices) {
      const item = getItemAtIndex(index);
      if (!item?.has_thumbnail) continue;
      retained.add(item.thumbnail_hash);
      if (!activationSinceByHash.has(item.thumbnail_hash)) {
        activationSinceByHash.set(item.thumbnail_hash, now);
      }
    }

    for (const hash of activationSinceByHash.keys()) {
      if (!retained.has(hash)) {
        activationSinceByHash.delete(hash);
      }
    }

    const processActivationWindow = () => {
      preloadTimerRef.current = null;
      const currentNow = performance.now();
      let nextDelay = Number.POSITIVE_INFINITY;

      for (const hash of retained) {
        const activeSince = activationSinceByHash.get(hash);
        if (activeSince == null) continue;
        const remainingMs = PRELOAD_START_DELAY_MS - (currentNow - activeSince);
        if (remainingMs <= 0) {
          store.ensure(hash, mediaThumbnailUrl(hash));
        } else if (remainingMs < nextDelay) {
          nextDelay = remainingMs;
        }
      }

      store.sweep(retained);

      if (Number.isFinite(nextDelay)) {
        preloadTimerRef.current = globalThis.setTimeout(processActivationWindow, Math.max(1, nextDelay));
      }
    };

    processActivationWindow();

    return () => {
      if (preloadTimerRef.current != null) {
        globalThis.clearTimeout(preloadTimerRef.current);
        preloadTimerRef.current = null;
      }
    };
  }, [activationRange.renderedIndices, getItemAtIndex, textureStoreReadyVersion]);

  // Build and push snapshot on layout/prop changes (not scroll — scroll uses imperative path)
  useEffect(() => {
    const renderer = sceneRendererRef.current;
    const controller = scrollControllerRef.current;
    if (!renderer || !controller) return;

    const scrollTop = committedScrollTopRef.current;
    const snapshot = buildSnapshot(scrollTop, buildParams, controller);
    latestTilesRef.current = snapshot.tiles;
    renderer.setSnapshot(snapshot);
  }, [buildParams, sceneRendererReadyVersion]);

  useEffect(() => {
    const controller = scrollControllerRef.current;
    if (!controller) return;
    controller.setMetrics(layoutIndex.getTotalHeight(), containerSize.height);
  }, [containerSize.height, layoutIndex]);

  // Load-more check (debounced via scrollTopForReact)
  useEffect(() => {
    if (!onLoadMore) return;
    if (loadMorePendingRef.current) return;
    if (layoutIndex.getTotalHeight() - scrollTopForReact - containerSize.height < LOAD_MORE_THRESHOLD_PX) {
      loadMorePendingRef.current = true;
      onLoadMore();
    }
  }, [scrollTopForReact, containerSize.height, layoutIndex, onLoadMore]);

  // First paint notification
  useEffect(() => {
    if (firstPaintNotifiedRef.current) return;
    if (latestTilesRef.current.length === 0) return;
    if (!sceneRendererRef.current) return;
    const hasVisible = latestTilesRef.current.some((t) => t.isVisible);
    if (!hasVisible) return;
    firstPaintNotifiedRef.current = true;
    onFirstPaint?.();
  }, [onFirstPaint, sceneRendererReadyVersion, buildParams]);

  const hitTest = useCallback((localX: number, localY: number) => {
    const tiles = latestTilesRef.current;
    for (let i = tiles.length - 1; i >= 0; i -= 1) {
      const tile = tiles[i];
      if (
        localX >= tile.position.x
        && localX <= tile.position.x + tile.position.w
        && localY >= tile.position.y
        && localY <= tile.position.y + tile.position.h
      ) {
        return tile;
      }
    }
    return null;
  }, []);

  const handleWheel = useCallback((event: WheelEvent) => {
    if (!interactive) return;
    event.preventDefault();
    scrollControllerRef.current?.handleWheel(event.deltaY, event.deltaMode);
  }, [interactive]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !interactive) return;

    host.addEventListener('wheel', handleWheel, { passive: false });
    return () => host.removeEventListener('wheel', handleWheel);
  }, [handleWheel, interactive]);

  const handlePointerDown = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const host = hostRef.current;
    const controller = scrollControllerRef.current;
    if (!host || !controller) return;
    const rect = host.getBoundingClientRect();
    const localX = event.clientX - rect.left;
    const localY = event.clientY - rect.top;
    if (controller.beginPointerInteraction(localX, localY, containerSize.width)) {
      host.setPointerCapture(event.pointerId);
      return;
    }

    const hit = hitTest(localX, localY);
    pointerDownRef.current = {
      pointerId: event.pointerId,
      x: localX,
      y: localY,
      index: hit?.index ?? null,
    };
  }, [containerSize.width, hitTest]);

  const handlePointerMove = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const host = hostRef.current;
    const controller = scrollControllerRef.current;
    if (!host || !controller) return;
    const rect = host.getBoundingClientRect();
    const localX = event.clientX - rect.left;
    const localY = event.clientY - rect.top;

    if (controller.handlePointerMove(localY, containerSize.width)) {
      return;
    }

    if (pointerDownRef.current && distance(pointerDownRef.current.x, pointerDownRef.current.y, localX, localY) > 6) {
      pointerDownRef.current = null;
    }
  }, [containerSize.width]);

  const handlePointerUp = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const host = hostRef.current;
    if (!host) {
      scrollControllerRef.current?.endPointerInteraction();
      pointerDownRef.current = null;
      return;
    }
    if (host.hasPointerCapture(event.pointerId)) {
      host.releasePointerCapture(event.pointerId);
    }
    scrollControllerRef.current?.endPointerInteraction();

    const pointerDown = pointerDownRef.current;
    pointerDownRef.current = null;
    if (!pointerDown || pointerDown.pointerId !== event.pointerId) return;

    const rect = host.getBoundingClientRect();
    const localX = event.clientX - rect.left;
    const localY = event.clientY - rect.top;
    const hit = hitTest(localX, localY);
    if (!hit || hit.index !== pointerDown.index) return;
    const item = getItemAtIndex(hit.index);
    if (!item) return;
    onTileClick?.(hit.index, item);
  }, [getItemAtIndex, hitTest, onTileClick]);

  return (
    <div
      ref={hostRef}
      className={styles.webglHost}
      onPointerDown={interactive ? handlePointerDown : undefined}
      onPointerMove={interactive ? handlePointerMove : undefined}
      onPointerUp={interactive ? handlePointerUp : undefined}
      onPointerLeave={() => {
        pointerDownRef.current = null;
        scrollControllerRef.current?.endPointerInteraction();
      }}
    >
      <div className={styles.fpsCounter}>{fps} FPS</div>
    </div>
  );
}
