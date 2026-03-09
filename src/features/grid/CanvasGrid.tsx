import {
  useRef,
  useState,
  useEffect,
  useMemo,
  RefObject,
} from 'react';
import { MasonryImageItem } from './shared';
import { VideoScrubOverlay } from './VideoScrubOverlay';
import { mediaFileUrl } from '../../shared/lib/mediaUrl';
import { imageDrag } from '../../shared/lib/imageDrag';
import type { GridViewMode, GridEmptyContext } from './runtime';
import {
  type LayoutItem,
} from './layoutMath';
import { useGridLayoutEngine } from './layout/gridLayoutEngine';
import { useEstimatedGridTotalHeight } from './layout/useEstimatedGridTotalHeight';
import {
  type WaterfallSeenState,
} from './layout/canvasVisibilityPlan';
import { hasSameLayoutGeometry } from './renderer/canvasGridPrimitives';
import { useCanvasRedrawScheduler } from './renderer/useCanvasRedrawScheduler';
import { useCanvasHoverInteractions } from './renderer/useCanvasHoverInteractions';
import { useThumbnailPipelineLifecycle } from './media/useThumbnailPipelineLifecycle';
import { useCanvasViewport } from './renderer/useCanvasViewport';
import { useCanvasDragInteractions } from './renderer/useCanvasDragInteractions';
import { useCanvasBaseDraw } from './renderer/useCanvasBaseDraw';
import { useCanvasOverlayDraw } from './renderer/useCanvasOverlayDraw';
import { useCanvasHitTesting } from './renderer/useCanvasHitTesting';
import { useCanvasScrollAnchor } from './renderer/useCanvasScrollAnchor';
import { useCanvasLoadMore } from './renderer/useCanvasLoadMore';
import { useCanvasClickInteractions } from './renderer/useCanvasClickInteractions';
import { useCanvasPopAnimation } from './renderer/useCanvasPopAnimation';
import type { GridDebugStats } from './renderer/canvasGridDebug';
import { CanvasGridDebugHud } from './renderer/CanvasGridDebugHud';
import { HoverPreviewPortal } from './renderer/HoverPreviewPortal';
import { CanvasGridEmptyState } from './components/CanvasGridEmptyState';

const ZOOM_BTN_SIZE = 24;
const LOAD_MORE_THRESHOLD = 500;
const GRID_DEBUG_SAMPLE_MS = 300;

function isGridDebugEnabled(): boolean {
  if (typeof window === 'undefined') return false;
  try {
    const params = new URLSearchParams(window.location.search);
    if (params.get('gridDebug') === '1') return true;
    return window.localStorage.getItem('picto:gridDebug') === '1';
  } catch {
    return false;
  }
}

const GRID_DEBUG_ENABLED = isGridDebugEnabled();

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
}: CanvasGridProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null);
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayCtxRef = useRef<CanvasRenderingContext2D | null>(null);
  const hoveredTileRef = useRef<number | null>(null);
  const isScrollingRef = useRef(false);
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
  const [debugStats, setDebugStats] = useState<GridDebugStats | null>(null);
  const perfRef = useRef<{
    frames: number;
    drawMsTotal: number;
    visMsTotal: number;
    slowFrames: number;
    sampleStart: number;
    lastFrameAt: number;
    baseFrames: number;
    overlayFrames: number;
  }>({
    frames: 0,
    drawMsTotal: 0,
    visMsTotal: 0,
    slowFrames: 0,
    sampleStart: performance.now(),
    lastFrameAt: 0,
    baseFrames: 0,
    overlayFrames: 0,
  });

  const themeRef = useRef<{
    primaryColor: string;
    textPrimary: string;
    textTertiary: string;
    placeholderBg: string;
    borderRadius: number;
    innerBorder: string;
  } | null>(null);
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
  const paddingX = 16;
  const {
    textHeight,
    renderImages,
    layout,
    bucketIndex,
  } = useGridLayoutEngine({
    images: layoutImages,
    layoutWidth,
    targetSize,
    gap,
    viewMode,
    showTileName,
    showResolution,
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
  const bucketIndexRef = useRef(bucketIndex);
  bucketIndexRef.current = bucketIndex;
  const waterfallVisibleIndicesRef = useRef<number[]>([]);
  const waterfallPrefetchIndicesRef = useRef<number[]>([]);
  const waterfallHitIndicesRef = useRef<number[]>([]);
  const waterfallSeenStateRef = useRef<WaterfallSeenState>({
    seen: new Uint32Array(0),
    token: 1,
  });

  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const prevLayoutRef = useRef(layout);
  const viewModeRef = useRef(viewMode);
  viewModeRef.current = viewMode;
  const imagesRef = useRef(renderImages);
  imagesRef.current = renderImages;
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

  useEffect(() => {
    if (!frozen) markDirty('both');
  }, [frozen, markDirty]);

  useEffect(() => { markDirty('base'); }, [renamingHash, markDirty]);

  const atlasRef = useThumbnailPipelineLifecycle({
    markDirty,
    isScrollingRef,
    pendingAtlasDirtyRef,
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
    viewModeRef,
    layoutRef,
    imagesRef,
    bucketIndexRef,
    waterfallVisibleIndicesRef,
    waterfallPrefetchIndicesRef,
    waterfallSeenStateRef,
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
    gridDebugEnabled: GRID_DEBUG_ENABLED,
    gridDebugSampleMs: GRID_DEBUG_SAMPLE_MS,
    setDebugStats,
    markDirty,
  });

  drawBaseRef.current = drawBase;

  useCanvasLoadMore({
    scrollContainerRef,
    onLoadMore,
    onLoadMoreRef,
    getScrollMetrics,
    layoutRef,
    threshold: LOAD_MORE_THRESHOLD,
  });

  useEffect(() => { markDirty('both'); }, [layout, markDirty]);
  useCanvasScrollAnchor({
    layout,
    prevLayoutRef,
    scrollContainerRef,
    getScrollMetrics,
  });

  useEffect(() => { markDirty('overlay'); }, [selectedHashes, markDirty]);
  useEffect(() => { markDirty('base'); }, [thumbnailFitMode, showExtension, showExtensionLabel, markDirty]);
  const { hitTest, isZoomButtonHit } = useCanvasHitTesting({
    canvasRef,
    layoutRef,
    viewModeRef,
    scrollTopRef,
    viewportHeightRef,
    bucketIndexRef,
    waterfallSeenStateRef,
    waterfallHitIndicesRef,
    textHeightRef,
    zoomBtnSize: ZOOM_BTN_SIZE,
  });

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
  } = useCanvasHoverInteractions({
    hitTest,
    isZoomButtonHit,
    imagesRef,
    layoutRef,
    canvasRef,
    scrollTopRef,
    textHeightRef,
    hoveredTileRef,
    marqueeActiveRef,
    markDirty,
    videoScrubIdxRef,
  });
  dismissHoverPreviewRef.current = () => {
    clearPendingHoverTimers();
    if (hoveredTileRef.current != null) {
      hoveredTileRef.current = null;
    }
    setHoverPreview((prev) => (prev ? null : prev));
    markDirty('overlay');
  };
  dismissVideoScrubRef.current = () => {
    clearPendingVideoScrubTimer();
    clearVideoScrubIndex();
    setVideoScrub((prev) => (prev ? null : prev));
  };
  const {
    reorderDragRef,
    handlePointerDown,
    handleCanvasDragOver,
    handleCanvasDrop,
    handleCanvasDragLeave,
    clearDragState,
  } = useCanvasDragInteractions({
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
    bucketIndexRef,
    waterfallSeenStateRef,
    waterfallHitIndicesRef,
    reorderModeRef,
    dragDisabledRef,
    onReorderRef,
    markDirty,
  });
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
    perfRef,
    gridDebugEnabled: GRID_DEBUG_ENABLED,
  });
  drawOverlayRef.current = drawOverlay;
  useEffect(() => {
    return imageDrag.onNativeDragEnd(clearDragState);
  }, [clearDragState]);

  const { handleClick } = useCanvasClickInteractions({
    hitTest,
    isZoomButtonHit,
    imagesRef,
    onImageClickRef,
    showHoverPreview,
  });

  useCanvasPopAnimation({
    popHash,
    scrollContainerRef,
    onPopComplete,
    getScrollMetrics,
    layoutRef,
    imagesRef,
  });

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
      <div ref={containerRef}>
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

  const canvasSize = Math.min(canvasHeight, layout.totalHeight) || '100%';

  return (
    <div ref={containerRef} data-canvas-grid-root>
      <div style={{ position: 'relative', height: estimatedTotalHeight, width: '100%' }}>
        <div style={{ position: 'sticky', top: 0 }}>
          <canvas
            ref={canvasRef}
            onPointerDown={handlePointerDown}
            onClick={handleClick}
            onMouseMove={handleMouseMove}
            onMouseLeave={handleMouseLeave}
            onDragStart={(e: React.DragEvent) => e.preventDefault()}
            onDragOver={handleCanvasDragOver}
            onDrop={handleCanvasDrop}
            onDragLeave={handleCanvasDragLeave}
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
      {GRID_DEBUG_ENABLED && debugStats && <CanvasGridDebugHud debugStats={debugStats} />}
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
