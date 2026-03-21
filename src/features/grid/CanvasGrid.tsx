import {
  useRef,
  useEffect,
  useMemo,
  useCallback,
  RefObject,
} from 'react';
import { useComputedColorScheme } from '@mantine/core';
import { isVideoMime, MasonryImageItem } from './shared';
import { VideoScrubOverlay } from './VideoScrubOverlay';
import { mediaFileUrl } from '../../shared/lib/mediaUrl';
import { imageDrag } from '../../shared/lib/imageDrag';
import type { GridViewMode, GridEmptyContext } from './runtime';
import {
  type LayoutItem,
} from './layoutMath';
import { useWaterfallLayoutWorker } from './hooks/useWaterfallLayoutWorker';
import { computeTextHeight } from './gridLayout';
import { useEstimatedGridTotalHeight } from './layout/useEstimatedGridTotalHeight';
import { hasSameLayoutGeometry } from './renderer/canvasGridPrimitives';
import { useCanvasRedrawScheduler } from './renderer/useCanvasRedrawScheduler';
import { useCanvasPointerInteractions } from './renderer/useCanvasPointerInteractions';
import { useThumbnailPipelineLifecycle } from '../../shared/lib/canvas/useThumbnailPipelineLifecycle';
import type { ThumbnailPipeline } from '../../shared/lib/canvas/thumbnailPipeline';
import { useCanvasViewport } from './renderer/useCanvasViewport';
import { useCanvasBaseDraw } from './renderer/useCanvasBaseDraw';
import { useCanvasOverlayDraw } from './renderer/useCanvasOverlayDraw';
import { hitTestCanvasTile } from './renderer/canvasHitTesting';
import { HoverPreviewPortal } from './renderer/HoverPreviewPortal';
import { CanvasGridEmptyState } from './components/CanvasGridEmptyState';
import { createIdleCanvasScrollState } from '../../shared/lib/canvas/scrollState';

const ZOOM_BTN_SIZE = 24;
const LOAD_MORE_THRESHOLD = 500;

interface CanvasGridProps {
  images: MasonryImageItem[];
  targetSize: number;
  gap: number;
  viewMode: GridViewMode;
  selectedHashes: Set<string>;
  searchTags?: string[];
  onImageClick: (image: MasonryImageItem, event: React.MouseEvent) => void;
  onImport: () => void;
  onImportFolder?: () => void;
  onContainerWidthChange?: (width: number) => void;
  showEmptyState?: boolean;
  /** Context for empty state messaging */
  emptyContext?: GridEmptyContext;
  onLoadMore?: () => void;
  scrollContainerRef?: RefObject<HTMLDivElement | null>;
  popHash?: string | null;
  onPopComplete?: () => void;
  frozen?: boolean;
  marqueeActive?: boolean;
  showTileName?: boolean;
  showResolution?: boolean;
  showExtension?: boolean;
  showExtensionLabel?: boolean;
  /** Marquee rect in scroll-content space (set by ImageGrid during box selection) */
  marqueeRect?: { left: number; top: number; width: number; height: number } | null;
  /** Set of hashes currently hit by marquee (for visual highlight during drag) */
  marqueeHitHashes?: Set<string> | null;
  /** Refs for marquee data — updated directly during drag without React re-renders */
  marqueeRectRef?: React.RefObject<{ left: number; top: number; width: number; height: number } | null>;
  marqueeHitHashesRef?: React.RefObject<Set<string> | null>;
  /** Ref that receives a function to request an overlay-lane redraw (e.g. from marquee drag) */
  scheduleRedrawRef?: React.MutableRefObject<(() => void) | null>;
  /** Called when layout positions change (for parent hit-testing e.g. marquee, context menu) */
  onLayoutChange?: (positions: LayoutItem[]) => void;
  /** Enable drag-to-reorder mode (manual sort within a folder) */
  reorderMode?: boolean;
  /** Called on drop when reorder drag completes */
  onReorder?: (movedHashes: string[], targetIndex: number) => void;
  /** Total item count for scroll height estimation (prevents scrollbar jitter on batch load) */
  totalCount?: number | null;
  /** Optional slim lookahead sample (next-page metadata only; never rendered). */
  estimateSampleImages?: MasonryImageItem[];
  /** Disable drag initiation for scoped interactions that should be read-only. */
  dragDisabled?: boolean;
  thumbnailFitMode?: 'cover' | 'contain';
  /** Hash of the file currently being renamed inline — suppresses canvas name text for that tile */
  renamingHash?: string | null;
  /** Scope identity for navigation; changes here must disable scroll-anchor preservation. */
  scrollAnchorScopeKey?: string;
  /** Whether same-scope scroll preservation and auto-scroll behaviors are allowed. */
  preserveScrollBehaviors?: boolean;
  /** Visual inset above the grid content without shifting the scroll shell origin. */
  topInset?: number;
  /** Shared thumbnail atlas that can survive shell remounts. */
  atlasRef?: React.MutableRefObject<ThumbnailPipeline | null>;
  /** External ref updated with the dismiss-hover-preview function (for parent dismiss triggers). */
  dismissHoverPreviewRef?: React.MutableRefObject<() => void>;
  /** External ref updated with the dismiss-video-scrub function (for parent dismiss triggers). */
  dismissVideoScrubRef?: React.MutableRefObject<() => void>;
  /** Horizontal content padding (default: 4). Increases padding to center narrower content. */
  contentPaddingX?: number;
}

export interface CanvasGridHandle {
  /** Get the current layout positions array (for marquee hit testing) */
  getLayoutPositions(): LayoutItem[];
  /** Get the images array */
  getImages(): MasonryImageItem[];
  /** Request a canvas redraw (e.g. from marquee drag without React state) */
  scheduleRedraw(): void;
}

export function CanvasGrid({
  images,
  targetSize,
  gap,
  viewMode,
  selectedHashes,
  searchTags,
  onImageClick,
  onImport,
  onImportFolder,
  onContainerWidthChange,
  showEmptyState = true,
  emptyContext = 'default',
  onLoadMore,
  scrollContainerRef,
  popHash,
  onPopComplete,
  frozen = false,
  marqueeActive = false,
  showTileName = true,
  showResolution = true,
  showExtension = true,
  showExtensionLabel = true,
  marqueeRect = null,
  marqueeHitHashes = null,
  marqueeRectRef: marqueeRectRefProp,
  marqueeHitHashesRef: marqueeHitHashesRefProp,
  scheduleRedrawRef,
  onLayoutChange,
  reorderMode = false,
  onReorder,
  totalCount = null,
  estimateSampleImages = [],
  dragDisabled = false,
  thumbnailFitMode = 'cover',
  renamingHash = null,
  scrollAnchorScopeKey = 'default',
  preserveScrollBehaviors = true,
  topInset = 0,
  atlasRef: sharedAtlasRef,
  dismissHoverPreviewRef: externalDismissRef,
  dismissVideoScrubRef: externalVideoScrubDismissRef,
  contentPaddingX,
}: CanvasGridProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null);
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayCtxRef = useRef<CanvasRenderingContext2D | null>(null);
  const hoveredTileRef = useRef<number | null>(null);
  const idleScrollState = createIdleCanvasScrollState();
  const isScrollingRef = useRef(false);
  const scrollPhaseRef = useRef(idleScrollState.phase);
  const scrollDirectionRef = useRef(idleScrollState.direction);
  const scrollVelocityRef = useRef(idleScrollState.velocityPxPerSec);
  const reorderModeRef = useRef(reorderMode);
  reorderModeRef.current = reorderMode;
  const dragDisabledRef = useRef(dragDisabled);
  dragDisabledRef.current = dragDisabled;
  const renamingHashRef = useRef(renamingHash);
  renamingHashRef.current = renamingHash;
  const onReorderRef = useRef(onReorder);
  onReorderRef.current = onReorder;

  const lastVisibleRef = useRef<{
    startIdx: number;
    endIdx: number;
    visibleIndices: number[] | null;
    visibleIterEnd: number;
    scrollTop: number;
    cssH: number;
    th: number;
    br: number;
  } | null>(null);

  const pendingAtlasDirtyRef = useRef(false);
  const dismissHoverPreviewRef = useRef<() => void>(() => {});
  const dismissVideoScrubRef = useRef<() => void>(() => {});
  const themeRef = useRef<{
    primaryColor: string;
    textPrimary: string;
    textTertiary: string;
    placeholderBg: string;
    borderRadius: number;
    innerBorder: string;
  } | null>(null);
  const colorScheme = useComputedColorScheme('dark');
  const frozenRef = useRef(frozen);
  frozenRef.current = frozen;
  const drawBaseRef = useRef<() => void>(() => {});
  const drawOverlayRef = useRef<() => void>(() => {});
  const { markDirty } = useCanvasRedrawScheduler({
    frozenRef,
    drawBaseRef,
    drawOverlayRef,
  });

  const {
    containerRef,
    containerWidth,
    layoutWidth,
    canvasHeight,
    canvasTopOffset,
    frozenCanvasWidth,
    getScrollMetrics,
    scrollTopRef,
    viewportHeightRef,
  } = useCanvasViewport({
    scrollContainerRef,
    onContainerWidthChange,
    frozen,
    markDirty,
    isScrollingRef,
    scrollPhaseRef,
    scrollDirectionRef,
    scrollVelocityRef,
    pendingAtlasDirtyRef,
    dismissHoverPreviewRef,
    dismissVideoScrubRef,
  });

  // Keep layout input reference stable unless tile geometry truly changed.
  // This avoids expensive waterfall re-layouts when only metadata fields update.
  const stableLayoutImagesRef = useRef<MasonryImageItem[] | null>(null);
  const layoutImages = useMemo(() => {
    const prev = stableLayoutImagesRef.current;
    if (prev && hasSameLayoutGeometry(prev, images)) {
      return prev;
    }
    stableLayoutImagesRef.current = images;
    return images;
  }, [images]);

  // Horizontal padding prevents clipping of edge drop indicators
  const paddingX = contentPaddingX ?? 4;
  const textHeight = computeTextHeight(showTileName, showResolution);
  const { renderImages, layout, bucketIndex } = useWaterfallLayoutWorker({
    images: layoutImages,
    layoutWidth,
    targetSize,
    gap,
    viewMode,
    textHeight,
    paddingX,
  });

  // Estimate total scroll height while batches stream in.
  // - grid: deterministic from totalCount/columns.
  // - justified/waterfall: average-per-item projection, optionally enriched with
  //   one-page lookahead metadata (never rendered).
  const estimatedTotalHeight = useEstimatedGridTotalHeight({
    exactHeight: layout.totalHeight,
    renderImages,
    estimateSampleImages,
    totalCount,
    viewMode,
    layoutWidth,
    targetSize,
    gap,
    textHeight,
    paddingX,
  });
  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const bucketIndexRef = useRef(bucketIndex);
  bucketIndexRef.current = bucketIndex;
  const prevLayoutRef = useRef(layout);
  const viewModeRef = useRef(viewMode);
  viewModeRef.current = viewMode;
  const thumbnailFitModeRef = useRef(thumbnailFitMode);
  thumbnailFitModeRef.current = thumbnailFitMode;
  const imagesRef = useRef(renderImages);
  imagesRef.current = renderImages;

  // Dismiss video scrub when layout changes (images added/removed/reordered)
  const prevRenderImagesRef = useRef(renderImages);
  if (prevRenderImagesRef.current !== renderImages) {
    prevRenderImagesRef.current = renderImages;
    dismissVideoScrubRef.current();
  }
  const selectedHashesRef = useRef(selectedHashes);
  selectedHashesRef.current = selectedHashes;
  const onImageClickRef = useRef(onImageClick);
  onImageClickRef.current = onImageClick;
  const onLoadMoreRef = useRef(onLoadMore);
  onLoadMoreRef.current = onLoadMore;
  const marqueeActiveRef = useRef(marqueeActive);
  marqueeActiveRef.current = marqueeActive;
  const marqueeRectRef = useRef(marqueeRect);
  marqueeRectRef.current = marqueeRect;
  const marqueeHitHashesRef = useRef(marqueeHitHashes);
  marqueeHitHashesRef.current = marqueeHitHashes;
  const textHeightRef = useRef(textHeight);
  textHeightRef.current = textHeight;
  const showTileNameRef = useRef(showTileName);
  showTileNameRef.current = showTileName;
  const showResolutionRef = useRef(showResolution);
  showResolutionRef.current = showResolution;
  const showExtensionRef = useRef(showExtension);
  showExtensionRef.current = showExtension;
  const showExtensionLabelRef = useRef(showExtensionLabel);
  showExtensionLabelRef.current = showExtensionLabel;
  const videoScrubIdxRef = useRef<number | null>(null);

  useEffect(() => {
    onLayoutChange?.(layout.positions);
  }, [layout, onLayoutChange]);

  useEffect(() => {
    if (scheduleRedrawRef) {
      scheduleRedrawRef.current = () => markDirty('overlay');
      return () => { scheduleRedrawRef.current = null; };
    }
  }, [markDirty, scheduleRedrawRef]);

  const frozenCanvasHeightRef = useRef<number | null>(null);
  useEffect(() => {
    if (frozen) {
      // Capture canvas height at freeze time to prevent vertical stretching
      const canvas = canvasRef.current;
      frozenCanvasHeightRef.current = canvas ? canvas.clientHeight : null;
      // Dismiss hover preview and video scrub when grid freezes (e.g. viewer opens)
      dismissHoverPreviewRef.current();
      dismissVideoScrubRef.current();
    } else {
      frozenCanvasHeightRef.current = null;
      markDirty('both');
    }
  }, [frozen, markDirty]);

  useEffect(() => { markDirty('base'); }, [renamingHash, markDirty]);

  const atlasRef = useThumbnailPipelineLifecycle({
    markDirty,
    scrollPhaseRef,
    pendingAtlasDirtyRef,
    sharedAtlasRef,
    destroyOnUnmount: !sharedAtlasRef,
  });
  const drawBase = useCanvasBaseDraw({
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
  });

  drawBaseRef.current = drawBase;

  // -- load-more trigger (inlined from useCanvasLoadMore) --
  useEffect(() => {
    const scrollElement = scrollContainerRef?.current;
    if (!scrollElement || !onLoadMore) return;
    const onScroll = () => {
      const metrics = getScrollMetrics();
      if (metrics.localScrollTop + metrics.viewportHeight > layoutRef.current.totalHeight - LOAD_MORE_THRESHOLD) {
        onLoadMoreRef.current?.();
      }
    };
    scrollElement.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      scrollElement.removeEventListener('scroll', onScroll);
    };
  }, [getScrollMetrics, layoutRef, onLoadMore, onLoadMoreRef, scrollContainerRef]);

  useEffect(() => { markDirty('both'); }, [layout, markDirty]);
  // -- scroll anchor preservation (inlined from useCanvasScrollAnchor) --
  // Only fires on layout changes caused by resize (same images, different positions).
  // Skips when frozen (transition in progress) or when the images array changed
  // (scope transition — lifecycle hook handles scroll position).
  const prevAnchorImagesRef = useRef(renderImages);
  const prevScrollAnchorScopeKeyRef = useRef(scrollAnchorScopeKey);
  useEffect(() => {
    const prev = prevLayoutRef.current;
    const prevImages = prevAnchorImagesRef.current;
    const prevScopeKey = prevScrollAnchorScopeKeyRef.current;
    prevLayoutRef.current = layout;
    prevAnchorImagesRef.current = renderImages;
    prevScrollAnchorScopeKeyRef.current = scrollAnchorScopeKey;
    if (!preserveScrollBehaviors) return;
    if (prevScopeKey !== scrollAnchorScopeKey) return;
    if (frozenRef.current) return;
    if (prevImages !== renderImages) return;
    if (!prev || prev.positions === layout.positions) return;
    if (prev.positions.length !== layout.positions.length) return;

    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) return;
    const metrics = getScrollMetrics();
    const st = metrics.localScrollTop;
    const vh = metrics.viewportHeight;
    if (vh === 0) return;

    const viewportCenter = st + vh / 2;
    let anchorIdx = -1;
    const selectedHashes = selectedHashesRef.current;
    if (selectedHashes.size === 1) {
      const selectedHash = [...selectedHashes][0];
      const selectedIdx = prevImages.findIndex((img) => img.hash === selectedHash);
      if (selectedIdx >= 0 && selectedIdx < prev.positions.length) {
        const selectedPos = prev.positions[selectedIdx];
        const isSelectedVisible = selectedPos.y + selectedPos.h >= st && selectedPos.y <= st + vh;
        if (isSelectedVisible) {
          anchorIdx = selectedIdx;
        }
      }
    }

    if (anchorIdx < 0) {
      let bestDist = Infinity;
      for (let i = 0; i < prev.positions.length; i++) {
        const p = prev.positions[i];
        const tileCenter = p.y + p.h / 2;
        const dist = Math.abs(tileCenter - viewportCenter);
        if (dist < bestDist) {
          bestDist = dist;
          anchorIdx = i;
        }
      }
    }
    if (anchorIdx < 0 || anchorIdx >= layout.positions.length) return;

    const oldTileCenter = prev.positions[anchorIdx].y + prev.positions[anchorIdx].h / 2;
    const offsetInViewport = oldTileCenter - st;
    const newTileCenter = layout.positions[anchorIdx].y + layout.positions[anchorIdx].h / 2;
    const newScrollTop = newTileCenter - offsetInViewport;
    scrollEl.scrollTop = Math.max(0, metrics.canvasTopInScroll + newScrollTop);
  }, [getScrollMetrics, layout, preserveScrollBehaviors, prevLayoutRef, scrollAnchorScopeKey, scrollContainerRef]);

  useEffect(() => { markDirty('overlay'); }, [selectedHashes, markDirty]);
  useEffect(() => { markDirty('base'); }, [thumbnailFitMode, showExtension, showExtensionLabel, markDirty]);
  useEffect(() => { themeRef.current = null; markDirty('both'); }, [colorScheme, markDirty]);
  const hitTest = useCallback((clientX: number, clientY: number): number | null => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    return hitTestCanvasTile({
      positions: layoutRef.current.positions,
      mouseX: clientX - rect.left,
      mouseY: clientY - rect.top + scrollTopRef.current,
      scrollTop: scrollTopRef.current,
      viewportHeight: viewportHeightRef.current,
    });
  }, [canvasRef, layoutRef, scrollTopRef, viewportHeightRef]);

  const isZoomButtonHit = useCallback((clientX: number, clientY: number, tileIdx: number): boolean => {
    const canvas = canvasRef.current;
    if (!canvas) return false;
    const rect = canvas.getBoundingClientRect();
    const mx = clientX - rect.left;
    const my = clientY - rect.top + scrollTopRef.current;
    const pos = layoutRef.current.positions[tileIdx];
    if (!pos) return false;
    const imageHeight = pos.h - textHeightRef.current;
    const bgW = ZOOM_BTN_SIZE + 4;
    const bgH = ZOOM_BTN_SIZE + 2;
    const zx = pos.x + pos.w - bgW;
    const zy = pos.y + imageHeight - bgH;
    return mx >= zx && mx < zx + bgW && my >= zy && my < zy + bgH;
  }, [canvasRef, layoutRef, scrollTopRef, textHeightRef]);

  const {
    hoverPreview,
    setHoverPreview,
    showHoverPreview,
    handleMouseMove,
    handleMouseLeave,
    videoScrub,
    setVideoScrub,
    clearPendingHoverTimers,
    clearPendingVideoScrubTimer,
    clearVideoScrubIndex,
    dragJustEndedRef,
    reorderDragRef,
    handlePointerDown,
    clearDragState,
  } = useCanvasPointerInteractions({
    hitTest,
    isZoomButtonHit,
    canvasRef,
    scrollContainerRef,
    getScrollMetrics,
    imagesRef,
    selectedHashesRef,
    layoutRef,
    viewModeRef,
    viewportHeightRef,
    reorderModeRef,
    dragDisabledRef,
    onReorderRef,
    scrollTopRef,
    textHeightRef,
    hoveredTileRef,
    marqueeActiveRef,
    markDirty,
    videoScrubIdxRef,
    thumbnailFitModeRef,
  });
  const dismissFn = () => {
    clearPendingHoverTimers();
    if (hoveredTileRef.current != null) {
      hoveredTileRef.current = null;
    }
    setHoverPreview((prev) => (prev ? null : prev));
    markDirty('overlay');
  };
  dismissHoverPreviewRef.current = dismissFn;
  if (externalDismissRef) externalDismissRef.current = dismissFn;
  const dismissVideoScrubFn = () => {
    clearPendingVideoScrubTimer();
    clearVideoScrubIndex();
    setVideoScrub((prev) => (prev ? null : prev));
  };
  dismissVideoScrubRef.current = dismissVideoScrubFn;
  if (externalVideoScrubDismissRef) externalVideoScrubDismissRef.current = dismissVideoScrubFn;
  const drawOverlay = useCanvasOverlayDraw({
    lastVisibleRef,
    overlayCanvasRef,
    overlayCtxRef,
    themeRef,
    layoutRef,
    imagesRef,
    selectedHashesRef,
    hoveredTileRef,
    marqueeRectRefProp,
    marqueeRectRef,
    marqueeHitHashesRefProp,
    marqueeHitHashesRef,
    marqueeActiveRef,
    isScrollingRef,
    reorderDragRef,
    gap,
    zoomBtnSize: ZOOM_BTN_SIZE,
  });
  drawOverlayRef.current = drawOverlay;
  useEffect(() => {
    return imageDrag.onNativeDragEnd(clearDragState);
  }, [clearDragState]);

  // -- click interactions (inlined from useCanvasClickInteractions) --
  const handleClick = useCallback((e: React.MouseEvent) => {
    // Suppress the click that fires after a drag (native or internal) ends —
    // without this, releasing the mouse changes the selection to whatever is
    // under the cursor at drop time.
    if (dragJustEndedRef.current) {
      dragJustEndedRef.current = false;
      return;
    }

    const idx = hitTest(e.clientX, e.clientY);
    if (idx == null) return;
    const image = imagesRef.current[idx];
    if (!image) return;

    if (isZoomButtonHit(e.clientX, e.clientY, idx)) {
      if (!isVideoMime(image.mime) && !image.is_collection) showHoverPreview(image);
      return;
    }

    onImageClickRef.current(image, e);
  }, [dragJustEndedRef, hitTest, imagesRef, isZoomButtonHit, onImageClickRef, showHoverPreview]);

  // -- pop animation / scroll-into-view (inlined from useCanvasPopAnimation) --
  useEffect(() => {
    if (!preserveScrollBehaviors) return;
    if (!popHash) return;
    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) {
      onPopComplete?.();
      return;
    }

    const positions = layoutRef.current.positions;
    const imgs = imagesRef.current;
    const idx = imgs.findIndex((img) => img.hash === popHash);
    if (idx === -1 || !positions[idx]) {
      onPopComplete?.();
      return;
    }

    const pos = positions[idx];
    const metrics = getScrollMetrics();
    const viewportH = metrics.viewportHeight;
    const scrollTop = metrics.localScrollTop;

    if (pos.y < scrollTop || pos.y + pos.h > scrollTop + viewportH) {
      const targetLocalScroll = pos.y - viewportH / 2 + pos.h / 2;
      scrollEl.scrollTop = Math.max(0, metrics.canvasTopInScroll + targetLocalScroll);
    }
    onPopComplete?.();
  }, [getScrollMetrics, imagesRef, layoutRef, onPopComplete, popHash, preserveScrollBehaviors, scrollContainerRef]);

  // Not yet measured
  if (containerWidth === 0) {
    return <div ref={containerRef} style={{ minHeight: 1 }} />;
  }

  // Empty state — drop area
  if (renderImages.length === 0) {
    if (!showEmptyState) {
      return <div ref={containerRef} style={{ minHeight: 1 }} />;
    }
    return (
      <div ref={containerRef} style={{ position: 'relative', minHeight: 400 }}>
        <CanvasGridEmptyState
          emptyContext={emptyContext}
          searchTags={searchTags}
          onImport={onImport}
          onImportFolder={onImportFolder}
        />
      </div>
    );
  }

  const lockedCanvasWidth = frozen && frozenCanvasWidth ? `${frozenCanvasWidth}px` : '100%';

  // When content exceeds available space, use full viewport for sticky scrolling.
  // Otherwise, fill available space so marquee can extend below images.
  const contentHeight = layout.totalHeight + topInset;
  const availableHeight = Math.max(0, canvasHeight - canvasTopOffset);
  // Subtract topInset so the canvas fits inside the border-box container's content area
  const computedCanvasHeight = (contentHeight > availableHeight ? canvasHeight : Math.max(0, availableHeight - topInset)) || '100%';
  // Lock canvas height during freeze to prevent vertical stretching from diagonal window resize
  const canvasSize = (frozen && frozenCanvasHeightRef.current != null) ? frozenCanvasHeightRef.current : computedCanvasHeight;

  return (
    <div ref={containerRef} data-canvas-grid-root data-grid-surface-root>
      <div style={{ position: 'relative', height: Math.max(estimatedTotalHeight + topInset, typeof canvasSize === 'number' ? canvasSize : 0), width: '100%', paddingTop: topInset, boxSizing: 'border-box' }}>
        <div style={{ position: 'sticky', top: 0 }}>
          <canvas
            ref={canvasRef}
            onPointerDown={handlePointerDown}
            onClick={handleClick}
            onMouseMove={handleMouseMove}
            onMouseLeave={handleMouseLeave}
            onDragStart={(e: React.DragEvent) => e.preventDefault()}
            style={{
              width: lockedCanvasWidth,
              height: canvasSize,
              display: 'block',
              cursor: 'default',
            }}
          />
          <canvas
            ref={overlayCanvasRef}
            style={{
              position: 'absolute',
              inset: 0,
              width: lockedCanvasWidth,
              height: canvasSize,
              display: 'block',
              pointerEvents: 'none',
            }}
          />
        </div>
      </div>
      {hoverPreview && <HoverPreviewPortal {...hoverPreview} />}
      {videoScrub && (
        <VideoScrubOverlay
          tileRect={videoScrub.rect}
          src={mediaFileUrl(videoScrub.hash, videoScrub.mime)}
          duration={videoScrub.durationSec}
          onDismiss={() => {
            videoScrubIdxRef.current = null;
            setVideoScrub(null);
          }}
        />
      )}
    </div>
  );
}
