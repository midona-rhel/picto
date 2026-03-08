import {
  useRef,
  useState,
  useEffect,
  useCallback,
  useMemo,
  RefObject,
} from 'react';
import { createPortal } from 'react-dom';
import { IconPhoto, IconUpload, IconFolderPlus } from '@tabler/icons-react';
import { TextButton } from '../../shared/components/TextButton';
import { StateBlock, StateActions } from '../../shared/components/state';
import { MasonryImageItem, isVideoMime } from './shared';
import { VideoScrubOverlay, type VideoScrubRect } from './VideoScrubOverlay';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { createDragIcon } from '../../shared/lib/createDragIcon';
import { formatDuration } from '../../shared/lib/formatters';
import { imageDrag } from '../../shared/lib/imageDrag';
import { getCurrentWebview } from '#desktop/api';
import { ImageAtlas } from './imageAtlas';
import type { GridViewMode, GridEmptyContext } from './runtime';
import {
  computeTextHeight,
  TEXT_NAME_ROW_H,
} from './VirtualGrid';
import {
  BUCKET_SIZE,
  type LayoutItem,
} from './layoutMath';
import { useWaterfallLayoutWorker } from './hooks/useWaterfallLayoutWorker';

const DRAG_THRESHOLD_SQ = 25; // 5px²
const BADGE_HIDDEN_TYPES = new Set(['jpg', 'jpeg', 'png', 'webp']);
const PREVIEW_DELAY_MS = 200;
const VIDEO_SCRUB_DELAY_MS = 500;
const PREVIEW_INSET = 48;
const ZOOM_BTN_SIZE = 24;
const BADGE_H = 18;
const BADGE_FONT = '600 10px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const NAME_FONT = '13px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const INFO_FONT = '11px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
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

const SCROLLBAR_JITTER_PX = 20;

function useContainerWidth() {
  const [width, setWidth] = useState(0);
  const widthRef = useRef(0);
  const roRef = useRef<ResizeObserver | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hasMeasured = useRef(false);
  const ref = useCallback((el: HTMLDivElement | null) => {
    if (roRef.current) {
      roRef.current.disconnect();
      roRef.current = null;
    }
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    hasMeasured.current = false;
    if (el) {
      const ro = new ResizeObserver(([entry]) => {
        const rounded = Math.round(entry.contentRect.width);
        if (!hasMeasured.current) {
          hasMeasured.current = true;
          widthRef.current = rounded;
          setWidth(rounded);
          return;
        }
        const prev = widthRef.current;
        if (rounded === prev) return;

        // Apply real window resizes immediately to avoid transient stretch artifacts.
        // Only debounce tiny width oscillations (typically scrollbar jitter).
        if (Math.abs(rounded - prev) > SCROLLBAR_JITTER_PX) {
          widthRef.current = rounded;
          setWidth(rounded);
          return;
        }

        if (timerRef.current) clearTimeout(timerRef.current);
        timerRef.current = setTimeout(() => {
          if (rounded === widthRef.current) return;
          widthRef.current = rounded;
          setWidth(rounded);
        }, 90);
      });
      ro.observe(el);
      roRef.current = ro;
    }
  }, []);
  return { ref, width };
}

function mimeToExt(mime: string): string {
  const slash = mime.indexOf('/');
  if (slash < 0) return '';
  const sub = mime.slice(slash + 1).toLowerCase();
  const MAP: Record<string, string> = {
    'jpeg': 'jpg', 'png': 'png', 'gif': 'gif', 'webp': 'webp', 'svg+xml': 'svg',
    'mp4': 'mp4', 'webm': 'webm', 'quicktime': 'mov', 'x-matroska': 'mkv',
    'bmp': 'bmp', 'tiff': 'tiff', 'avif': 'avif', 'heic': 'heic',
  };
  return MAP[sub] ?? sub;
}

function getEmptyStateTitle(emptyContext: GridEmptyContext, hasSearchTags: boolean): string {
  if (hasSearchTags) return 'No results found';
  if (emptyContext === 'inbox') return 'Inbox is empty';
  if (emptyContext === 'uncategorized') return 'No uncategorized images';
  if (emptyContext === 'untagged') return 'No untagged images';
  if (emptyContext === 'smart-folder') return 'No matching images';
  if (emptyContext === 'folder') return 'This folder is empty';
  return 'No images';
}

function getEmptyStateDescription(emptyContext: GridEmptyContext, hasSearchTags: boolean): string {
  if (hasSearchTags) return 'Try different search terms or clear filters';
  if (emptyContext === 'inbox') return 'Run subscriptions to add new images to your inbox';
  if (emptyContext === 'uncategorized') return 'All your images are already assigned to folders';
  if (emptyContext === 'untagged') return 'All your images have been tagged';
  if (emptyContext === 'smart-folder') return 'Try adjusting the rules for this smart folder';
  if (emptyContext === 'folder') return 'Drag and drop files here, or import them below';
  return 'Drag and drop files here, or click the button below to import';
}

function isCollectionTile(image: MasonryImageItem): boolean {
  return image.is_collection === true;
}

function lowerBound(
  positions: LayoutItem[],
  target: number,
  selector: (item: LayoutItem) => number,
): number {
  let lo = 0;
  let hi = positions.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (selector(positions[mid]) < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** object-fit: cover equivalent for drawImage */
function drawImageCover(
  ctx: CanvasRenderingContext2D,
  bitmap: ImageBitmap,
  dx: number,
  dy: number,
  dw: number,
  dh: number,
): void {
  const srcAspect = bitmap.width / bitmap.height;
  const dstAspect = dw / dh;
  let sx: number, sy: number, sw: number, sh: number;
  if (srcAspect > dstAspect) {
    // Source is wider — crop horizontally
    sh = bitmap.height;
    sw = sh * dstAspect;
    sx = (bitmap.width - sw) / 2;
    sy = 0;
  } else {
    // Source is taller — crop vertically
    sw = bitmap.width;
    sh = sw / dstAspect;
    sx = 0;
    sy = (bitmap.height - sh) / 2;
  }
  ctx.drawImage(bitmap, sx, sy, sw, sh, dx, dy, dw, dh);
}

/** object-fit: contain equivalent — letterbox, no crop */
function drawImageContain(
  ctx: CanvasRenderingContext2D,
  bitmap: ImageBitmap,
  dx: number,
  dy: number,
  dw: number,
  dh: number,
): void {
  const scale = Math.min(dw / bitmap.width, dh / bitmap.height);
  const sw = bitmap.width * scale;
  const sh = bitmap.height * scale;
  const ox = dx + (dw - sw) / 2;
  const oy = dy + (dh - sh) / 2;
  ctx.drawImage(bitmap, 0, 0, bitmap.width, bitmap.height, ox, oy, sw, sh);
}

/** Draw a rounded-rect badge overlay */
function drawBadge(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
): void {
  ctx.font = BADGE_FONT;
  const metrics = ctx.measureText(text);
  const padH = 4;
  const w = metrics.width + padH * 2;
  const h = BADGE_H;
  const r = 4;

  // Background
  ctx.fillStyle = 'rgba(0, 0, 0, 0.65)';
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
  ctx.fill();

  // Text — explicitly set textAlign since Pass 6 leaves it as 'center'
  ctx.fillStyle = 'rgba(255, 255, 255, 0.80)';
  ctx.textAlign = 'left';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, x + padH, y + h / 2);
}

/**
 * Returns true when two image lists would produce identical tile geometry.
 * We intentionally ignore non-layout fields (name/rating/view_count/etc.).
 */
function hasSameLayoutGeometry(
  prev: MasonryImageItem[],
  next: MasonryImageItem[],
): boolean {
  if (prev === next) return true;
  if (prev.length !== next.length) return false;
  for (let i = 0; i < prev.length; i++) {
    const a = prev[i];
    const b = next[i];
    if (a.hash !== b.hash) return false;
    if (Math.abs(a.aspectRatio - b.aspectRatio) > 0.0001) return false;
  }
  return true;
}

/** Truncate text with ellipsis to fit maxWidth — cached to avoid repeated measureText calls */
const _truncCache = new Map<string, string>();

function truncateText(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string {
  const key = `${text}\0${Math.round(maxWidth)}`;
  const cached = _truncCache.get(key);
  if (cached !== undefined) return cached;

  let result: string;
  if (ctx.measureText(text).width <= maxWidth) {
    result = text;
  } else {
    // Linear scan from end — typically only a few chars need trimming
    let end = text.length - 1;
    while (end > 0 && ctx.measureText(text.slice(0, end) + '…').width > maxWidth) {
      end--;
    }
    result = text.slice(0, end) + '…';
  }

  _truncCache.set(key, result);
  // Cap cache size
  if (_truncCache.size > 5000) {
    const iter = _truncCache.keys();
    for (let i = 0; i < 2000; i++) iter.next();
    // Can't easily delete first N entries, just clear
    _truncCache.clear();
  }
  return result;
}

interface HoverPreviewData {
  hash: string;
  mime: string;
}

interface GridDebugStats {
  fps: number;
  drawMs: number;
  visMs: number;
  visibleTiles: number;
  prefetchedTiles: number;
  queueDepth: number;
  activeLoads: number;
  pendingBlurhash: number;
  cacheSize: number;
  slowFrames: number;
  diskSpeed: 'normal' | 'fast';
  baseRedraws: number;
  overlayRedraws: number;
}

const hoverPreviewLoadedCache = new Set<string>();

function HoverPreview({ hash, mime }: HoverPreviewData) {
  const fullUrl = mediaFileUrl(hash, mime);
  const [loaded, setLoaded] = useState(() => hoverPreviewLoadedCache.has(fullUrl));

  useEffect(() => {
    setLoaded(hoverPreviewLoadedCache.has(fullUrl));
  }, [fullUrl]);

  return createPortal(
    <div
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 200002,
        pointerEvents: 'none',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        backgroundColor: loaded ? 'rgba(0,0,0,0.6)' : 'transparent',
        transition: 'background-color 150ms ease',
      }}
    >
      <img
        src={fullUrl}
        alt=""
        onLoad={() => {
          hoverPreviewLoadedCache.add(fullUrl);
          setLoaded(true);
        }}
        style={{
          display: 'block',
          maxWidth: `calc(100vw - ${PREVIEW_INSET * 2}px)`,
          maxHeight: `calc(100vh - ${PREVIEW_INSET * 2}px)`,
          objectFit: 'contain',
          borderRadius: 8,
          boxShadow: '0 8px 48px rgba(0,0,0,0.7)',
          opacity: loaded ? 1 : 0,
          transition: 'opacity 150ms ease',
        }}
      />
    </div>,
    document.body,
  );
}

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

/** Resize a canvas backing buffer if needed. Returns [cssW, cssH, dpr]. */
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
  const { ref: measureContainerRef, width: containerWidth } = useContainerWidth();
  const containerElRef = useRef<HTMLDivElement | null>(null);
  const containerRef = useCallback((el: HTMLDivElement | null) => {
    containerElRef.current = el;
    measureContainerRef(el);
  }, [measureContainerRef]);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null);
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayCtxRef = useRef<CanvasRenderingContext2D | null>(null);
  const scrollTopRef = useRef(0);
  const viewportHeightRef = useRef(0);
  const hoveredTileRef = useRef<number | null>(null);
  const isScrollingRef = useRef(false);
  const dirtyRef = useRef<{ base: boolean; overlay: boolean }>({ base: false, overlay: false });
  const rafScheduledRef = useRef(false);
  const atlasRef = useRef<ImageAtlas | null>(null);
  const dragStateRef = useRef<{ hash: string; startX: number; startY: number; started: boolean } | null>(null);
  const reorderDragRef = useRef<{
    draggedHashes: string[];
    startX: number;
    startY: number;
    started: boolean;
    dropIndex: number | null;
    dropSide: 'left' | 'right' | null;
  } | null>(null);
  const draggedHashSetRef = useRef<Set<string> | null>(null);
  const reorderModeRef = useRef(reorderMode);
  reorderModeRef.current = reorderMode;
  const dragDisabledRef = useRef(dragDisabled);
  dragDisabledRef.current = dragDisabled;
  const renamingHashRef = useRef(renamingHash);
  renamingHashRef.current = renamingHash;
  const onReorderRef = useRef(onReorder);
  onReorderRef.current = onReorder;

  const autoScrollRef = useRef<{
    rafId: number | null;
    speed: number;               // px/frame, negative = up
    armed: boolean;              // true once cursor has been in the safe interior zone
  }>({ rafId: null, speed: 0, armed: false });

  const stopAutoScroll = useCallback(() => {
    const as = autoScrollRef.current;
    if (as.rafId != null) { cancelAnimationFrame(as.rafId); as.rafId = null; }
    as.speed = 0;
  }, []);

  const startAutoScroll = useCallback(() => {
    const as = autoScrollRef.current;
    if (as.rafId != null) return;
    const tick = () => {
      const scrollEl = scrollContainerRef?.current;
      if (!scrollEl || as.speed === 0) { as.rafId = null; return; }
      scrollEl.scrollTop += as.speed;
      as.rafId = requestAnimationFrame(tick);
    };
    as.rafId = requestAnimationFrame(tick);
  }, [scrollContainerRef]);
  const [canvasHeight, setCanvasHeight] = useState(0);
  const [frozenCanvasWidth, setFrozenCanvasWidth] = useState<number | null>(null);
  const [frozenLayoutWidth, setFrozenLayoutWidth] = useState<number | null>(null);
  const wasFrozenRef = useRef(false);
  const unfreezeSettleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  const [hoverPreview, setHoverPreview] = useState<HoverPreviewData | null>(null);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [videoScrub, setVideoScrub] = useState<{
    index: number;
    hash: string;
    mime: string;
    durationSec: number;
    rect: VideoScrubRect;
  } | null>(null);
  const videoScrubTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const videoScrubIdxRef = useRef<number | null>(null);

  const pendingAtlasDirtyRef = useRef(false);
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

  const textHeight = computeTextHeight(showTileName, showResolution);
  const layoutWidth = frozenLayoutWidth ?? containerWidth;

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
    renderImages,
    layout,
    bucketIndex,
  } = useWaterfallLayoutWorker({
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
  const estimateRef = useRef(0);
  const prevEstimateInputRef = useRef<{
    totalCount: number | null;
    imagesLen: number;
    viewMode: GridViewMode;
    layoutWidth: number;
  }>({
    totalCount: null,
    imagesLen: 0,
    viewMode,
    layoutWidth: 0,
  });
  const estimatedTotalHeight = useMemo(() => {
    const exactHeight = layout.totalHeight;
    const loadedAll = !totalCount || totalCount <= renderImages.length || renderImages.length === 0;
    if (loadedAll) {
      estimateRef.current = exactHeight;
      prevEstimateInputRef.current = { totalCount, imagesLen: renderImages.length, viewMode, layoutWidth };
      return exactHeight;
    }

    const innerWidth = Math.max(1, layoutWidth - 2 * paddingX);
    const clampAspect = (value: number): number => {
      if (!Number.isFinite(value) || value <= 0) return 1.5;
      return Math.min(8, Math.max(0.125, value));
    };
    const loadedSample = renderImages.slice(Math.max(0, renderImages.length - 220));
    const loadedHashes = new Set(loadedSample.map((item) => item.hash));
    const lookaheadSample = estimateSampleImages
      .slice(0, 120)
      .filter((item) => !loadedHashes.has(item.hash));
    const estimatePool = lookaheadSample.length > 0 ? [...loadedSample, ...lookaheadSample] : loadedSample;

    let projected = exactHeight;
    if (viewMode === 'grid') {
      const columnCount = Math.max(1, Math.round((innerWidth + gap) / (targetSize + gap)));
      const colWidth = Math.floor((innerWidth - (columnCount - 1) * gap) / columnCount);
      const cellH = colWidth + textHeight;
      const rows = Math.ceil(totalCount / columnCount);
      projected = rows > 0 ? rows * cellH + (rows - 1) * gap + 4 : 0;
    } else {
      let avgHeightPerItem = exactHeight / Math.max(1, renderImages.length);
      if (estimatePool.length > 0) {
        if (viewMode === 'waterfall') {
          const columnCount = Math.max(1, Math.round((innerWidth + gap) / (targetSize + gap)));
          const colWidth = Math.floor((innerWidth - (columnCount - 1) * gap) / columnCount);
          let sumH = 0;
          for (const image of estimatePool) {
            sumH += (colWidth / clampAspect(image.aspectRatio)) + textHeight;
          }
          const avgItemH = sumH / estimatePool.length;
          // Expected max column height approximation from per-item contribution.
          avgHeightPerItem = ((avgItemH + gap) / columnCount) * 1.04;
        } else if (viewMode === 'justified') {
          let y = 0;
          let rowStart = 0;
          while (rowStart < estimatePool.length) {
            let rowEnd = rowStart;
            let totalAspect = 0;
            while (rowEnd < estimatePool.length) {
              totalAspect += clampAspect(estimatePool[rowEnd].aspectRatio);
              rowEnd++;
              const rowWidth = totalAspect * targetSize + (rowEnd - rowStart - 1) * gap;
              if (rowWidth >= innerWidth) break;
            }
            const count = rowEnd - rowStart;
            const gapSpace = (count - 1) * gap;
            const rowHeight = (innerWidth - gapSpace) / Math.max(0.001, totalAspect);
            const finalHeight = Math.min(rowHeight, targetSize * 1.5);
            y += finalHeight + textHeight + gap;
            rowStart = rowEnd;
          }
          const totalH = Math.max(0, y - gap);
          avgHeightPerItem = Math.max(1, totalH / estimatePool.length);
        }
      }
      projected = Math.max(exactHeight, Math.round(avgHeightPerItem * totalCount));
    }
    projected = Math.max(exactHeight, projected);

    const prev = estimateRef.current || projected;
    const prevInput = prevEstimateInputRef.current;

    // Reset estimate on probable scope swap, view-mode swap, or major width/geometry reset.
    const modeChanged = prevInput.viewMode !== viewMode;
    const widthChanged = prevInput.layoutWidth > 0 && Math.abs(prevInput.layoutWidth - layoutWidth) > 12;
    const resetEstimate =
      modeChanged ||
      widthChanged ||
      prevInput.totalCount !== null &&
      totalCount < prevInput.totalCount &&
      renderImages.length <= prevInput.imagesLen;
    if (resetEstimate) {
      estimateRef.current = projected;
      prevEstimateInputRef.current = { totalCount, imagesLen: renderImages.length, viewMode, layoutWidth };
      return projected;
    }

    // Keep grid deterministic.
    // justified: allow controlled shrink to correct early over-estimates.
    // waterfall: allow smaller controlled correction, but clamp to exact height.
    let next = prev;
    if (viewMode === 'grid') {
      next = projected;
    } else if (viewMode === 'justified') {
      if (projected > prev) {
        const delta = projected - prev;
        if (delta < 48) next = projected;
        else next = prev + Math.max(24, Math.round(delta * 0.35));
      } else if (projected < prev) {
        const delta = prev - projected;
        if (delta < 48) next = projected;
        else next = prev - Math.max(24, Math.round(delta * 0.35));
      }
    } else if (viewMode === 'waterfall') {
      if (projected > prev) {
        const delta = projected - prev;
        if (delta < 48) next = projected;
        else next = prev + Math.max(24, Math.round(delta * 0.28));
      } else if (projected < prev) {
        const delta = prev - projected;
        if (delta < 48) next = projected;
        else next = prev - Math.max(16, Math.round(delta * 0.18));
      }
    }
    next = Math.max(next, exactHeight);

    estimateRef.current = next;
    prevEstimateInputRef.current = { totalCount, imagesLen: renderImages.length, viewMode, layoutWidth };
    return next;
  }, [layout.totalHeight, renderImages, estimateSampleImages, totalCount, viewMode, layoutWidth, targetSize, gap, textHeight]);
  const bucketIndexRef = useRef(bucketIndex);
  bucketIndexRef.current = bucketIndex;
  const waterfallVisibleIndicesRef = useRef<number[]>([]);
  const waterfallPrefetchIndicesRef = useRef<number[]>([]);
  const waterfallHitIndicesRef = useRef<number[]>([]);
  const waterfallSeenRef = useRef<Uint32Array>(new Uint32Array(0));
  const waterfallSeenTokenRef = useRef(1);

  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const prevLayoutRef = useRef(layout);
  const viewModeRef = useRef(viewMode);
  viewModeRef.current = viewMode;
  const frozenRef = useRef(frozen);
  frozenRef.current = frozen;
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

  const imagesByHash = useMemo(() => {
    const map = new Map<string, MasonryImageItem>();
    for (const img of renderImages) map.set(img.hash, img);
    return map;
  }, [renderImages]);
  const imagesByHashRef = useRef(imagesByHash);
  imagesByHashRef.current = imagesByHash;

  useEffect(() => {
    if (frozen || frozenLayoutWidth != null) return;
    onContainerWidthChange?.(containerWidth);
  }, [containerWidth, frozen, frozenLayoutWidth, onContainerWidthChange]);

  const getScrollMetrics = useCallback(() => {
    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) {
      return {
        localScrollTop: 0,
        canvasTopInScroll: 0,
        viewportHeight: 0,
      };
    }
    const viewportHeight = scrollEl.clientHeight;
    const globalScrollTop = scrollEl.scrollTop;
    const containerEl = containerElRef.current;
    if (!containerEl) {
      return {
        localScrollTop: globalScrollTop,
        canvasTopInScroll: 0,
        viewportHeight,
      };
    }

    // Convert scroll container coordinates into canvas-local coordinates.
    // This keeps draw math stable when content (e.g. subfolders) sits above the canvas.
    const scrollRect = scrollEl.getBoundingClientRect();
    const containerRect = containerEl.getBoundingClientRect();
    const canvasTopInScroll = globalScrollTop + (containerRect.top - scrollRect.top);
    const localScrollTop = Math.max(0, globalScrollTop - canvasTopInScroll);
    return {
      localScrollTop,
      canvasTopInScroll,
      viewportHeight,
    };
  }, [scrollContainerRef]);

  useEffect(() => {
    onLayoutChange?.(layout.positions);
  }, [layout, onLayoutChange]);

  // --- Redraw scheduling
  const drawBaseRef = useRef<() => void>(() => {});
  const drawOverlayRef = useRef<() => void>(() => {});

  const markDirty = useCallback((lanes: 'base' | 'overlay' | 'both') => {
    if (frozenRef.current) return;
    const d = dirtyRef.current;
    if (lanes === 'base' || lanes === 'both') d.base = true;
    if (lanes === 'overlay' || lanes === 'both') d.overlay = true;
    if (rafScheduledRef.current) return;
    rafScheduledRef.current = true;
    requestAnimationFrame(() => {
      rafScheduledRef.current = false;
      const dirty = dirtyRef.current;
      if (dirty.base) { dirty.base = false; drawBaseRef.current(); }
      if (dirty.overlay) { dirty.overlay = false; drawOverlayRef.current(); }
    });
  }, []);

  useEffect(() => {
    if (unfreezeSettleTimerRef.current) {
      clearTimeout(unfreezeSettleTimerRef.current);
      unfreezeSettleTimerRef.current = null;
    }
    if (frozen && !wasFrozenRef.current) {
      const w = canvasRef.current?.clientWidth ?? 0;
      const resolvedWidth = w > 0 ? w : (containerWidth > 0 ? containerWidth : 0);
      const frozenWidth = resolvedWidth > 0 ? resolvedWidth : null;
      setFrozenCanvasWidth(frozenWidth);
      setFrozenLayoutWidth(frozenWidth);
      // Dismiss video scrub overlay when entering detail view
      if (videoScrubTimerRef.current) {
        clearTimeout(videoScrubTimerRef.current);
        videoScrubTimerRef.current = null;
      }
      videoScrubIdxRef.current = null;
      setVideoScrub((prev) => (prev ? null : prev));
    } else if (!frozen && wasFrozenRef.current) {
      setFrozenCanvasWidth(null);
      // Keep old layout width briefly after release to avoid resize-stretch flash.
      unfreezeSettleTimerRef.current = setTimeout(() => {
        setFrozenLayoutWidth(null);
        unfreezeSettleTimerRef.current = null;
      }, 140);
    }
    wasFrozenRef.current = frozen;
  }, [frozen, containerWidth]);

  useEffect(() => {
    if (scheduleRedrawRef) {
      scheduleRedrawRef.current = () => markDirty('overlay');
      return () => { scheduleRedrawRef.current = null; };
    }
  }, [markDirty, scheduleRedrawRef]);

  // Clear drag indicators when native drag session ends
  useEffect(() => {
    return imageDrag.onNativeDragEnd(() => {
      stopAutoScroll();
      autoScrollRef.current.armed = false;
      draggedHashSetRef.current = null;
      reorderDragRef.current = null;
      markDirty('overlay');
    });
  }, [markDirty]);

  useEffect(() => {
    if (!frozen) markDirty('both');
  }, [frozen, markDirty]);

  useEffect(() => { markDirty('base'); }, [renamingHash, markDirty]);

  // --- ImageAtlas lifecycle
  useEffect(() => {
    const atlas = new ImageAtlas(() => {
      if (isScrollingRef.current) {
        pendingAtlasDirtyRef.current = true;
        return;
      }
      markDirty('base');
    });
    atlasRef.current = atlas;
    return () => {
      atlas.destroy();
      atlasRef.current = null;
    };
  }, [markDirty]);


  // --- Visible tile indices
  function getVisibleIndices(
    positions: LayoutItem[],
    scrollTop: number,
    viewportHeight: number,
    mode: GridViewMode,
  ): [number, number] {
    // Returns [startIdx, endIdx) — range of indices to draw
    if (positions.length === 0 || viewportHeight === 0) return [0, 0];

    const top = scrollTop;
    const bottom = scrollTop + viewportHeight;

    if (mode !== 'waterfall') {
      const start = lowerBound(positions, top, (p) => p.y + p.h);
      const end = lowerBound(positions, bottom, (p) => p.y);
      return [start, end];
    }

    // Waterfall: bucket index for O(visible) instead of O(N)
    const bi = bucketIndexRef.current;
    if (bi) {
      const startBucket = Math.floor(top / BUCKET_SIZE);
      const endBucket = Math.floor(bottom / BUCKET_SIZE);
      let minIdx = positions.length;
      let maxIdx = 0;
      for (let b = startBucket; b <= endBucket; b++) {
        const indices = bi.get(b);
        if (!indices) continue;
        for (const idx of indices) {
          const pos = positions[idx];
          if (pos.y + pos.h > top && pos.y < bottom) {
            if (idx < minIdx) minIdx = idx;
            if (idx > maxIdx) maxIdx = idx;
          }
        }
      }
      return minIdx <= maxIdx ? [minIdx, maxIdx + 1] : [0, 0];
    }

    // Fallback: linear scan
    let start = positions.length;
    let end = 0;
    for (let i = 0; i < positions.length; i++) {
      const pos = positions[i];
      if (pos.y + pos.h > top && pos.y < bottom) {
        if (i < start) start = i;
        if (i >= end) end = i + 1;
      }
    }
    return start <= end ? [start, end] : [0, 0];
  }

  function collectWaterfallIndices(
    positions: LayoutItem[],
    top: number,
    bottom: number,
    out: number[],
  ): number[] {
    out.length = 0;
    if (positions.length === 0 || bottom <= top) return out;
    const bi = bucketIndexRef.current;
    if (!bi) return out;

    let seen = waterfallSeenRef.current;
    if (seen.length < positions.length) {
      seen = new Uint32Array(positions.length);
      waterfallSeenRef.current = seen;
      waterfallSeenTokenRef.current = 1;
    }

    let token = waterfallSeenTokenRef.current + 1;
    if (token >= 0x7fffffff) {
      seen.fill(0);
      token = 1;
    }
    waterfallSeenTokenRef.current = token;

    const startBucket = Math.floor(top / BUCKET_SIZE);
    const endBucket = Math.floor(bottom / BUCKET_SIZE);
    for (let b = startBucket; b <= endBucket; b++) {
      const indices = bi.get(b);
      if (!indices) continue;
      for (let k = 0; k < indices.length; k++) {
        const idx = indices[k];
        if (seen[idx] === token) continue;
        const pos = positions[idx];
        if (!pos) continue;
        if (pos.y + pos.h <= top || pos.y >= bottom) continue;
        seen[idx] = token;
        out.push(idx);
      }
    }

    return out;
  }

  // --- Base lane: images, borders, badges, text
  function drawBase() {
    if (frozenRef.current) return;

    const canvas = canvasRef.current;
    if (!canvas) return;

    // Keep scroll coordinates in sync even when layout above the canvas changes
    // without a native scroll event (e.g. subfolder block mount/unmount).
    const metrics = getScrollMetrics();
    if (metrics.viewportHeight > 0) {
      viewportHeightRef.current = metrics.viewportHeight;
    }
    scrollTopRef.current = metrics.localScrollTop;

    // Lazy-init context — also re-acquire if canvas element was swapped (e.g. empty→non-empty)
    if (!ctxRef.current || ctxRef.current.canvas !== canvas) {
      ctxRef.current = canvas.getContext('2d', { alpha: true });
    }
    const ctx = ctxRef.current;
    if (!ctx) return;

    // Lazy-init theme colors
    if (!themeRef.current) {
      const s = getComputedStyle(document.documentElement);
      themeRef.current = {
        primaryColor: s.getPropertyValue('--color-primary').trim() || '#3297FF',
        textPrimary: s.getPropertyValue('--color-text-primary').trim() || 'rgba(255,255,255,0.92)',
        textTertiary: s.getPropertyValue('--color-text-tertiary').trim() || 'rgba(255,255,255,0.36)',
        placeholderBg: s.getPropertyValue('--tile-placeholder-bg').trim() || 'rgba(255,255,255,0.04)',
        borderRadius: parseInt(s.getPropertyValue('--tile-border-radius').trim()) || 4,
        innerBorder: s.getPropertyValue('--tile-inner-border').trim() || 'rgba(255,255,255,0.05)',
      };
    }
    const theme = themeRef.current;

    const atlas = atlasRef.current;
    if (!atlas) return;
    atlas.setScrolling(isScrollingRef.current);

    const t0 = import.meta.env.DEV ? performance.now() : 0;

    // Reset per-frame decode budget and process queued blurhash decodes
    atlas.resetFrameBudget();

    const positions = layoutRef.current.positions;
    const imgs = imagesRef.current;
    const scrollTop = scrollTopRef.current;
    const vh = viewportHeightRef.current;
    const isScrolling = isScrollingRef.current;

    // Update atlas viewport for priority sorting
    atlas.setViewport(scrollTop, vh);

    // DPR-aware dimensions
    const dpr = window.devicePixelRatio || 1;
    const [cssW, cssH] = ensureCanvasSize(canvas, dpr);

    // Reset transform and clear
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);

    if (positions.length === 0) {
      lastVisibleRef.current = null;
      return;
    }

    const mode = viewModeRef.current;
    const isWaterfall = mode === 'waterfall';
    // Visible range (list for waterfall, contiguous range for fixed layouts)
    const [startIdx, endIdx] = getVisibleIndices(positions, scrollTop, vh, mode);
    const visibleIndices = isWaterfall
      ? collectWaterfallIndices(positions, scrollTop, scrollTop + vh, waterfallVisibleIndicesRef.current)
      : null;
    const visibleIterEnd = visibleIndices
      ? visibleIndices.length
      : Math.max(0, Math.min(endIdx, positions.length) - startIdx);

    const tVis = import.meta.env.DEV ? performance.now() : 0;

    const br = theme.borderRadius;
    const th = textHeightRef.current;

    // Store visible-indices for the overlay lane
    lastVisibleRef.current = { startIdx, endIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br };

    const now = performance.now();
    let hasActiveCrossfade = false;

    // Pass 1: Images (placeholders -> blurhashes -> thumbnails)
    for (let n = 0; n < visibleIterEnd; n++) {
      const i = visibleIndices ? visibleIndices[n] : startIdx + n;
      const pos = positions[i];
      const image = imgs[i];
      if (!image) continue;
      const drawY = pos.y - scrollTop;
      const imageHeight = pos.h - th;
      if (drawY + pos.h < 0 || drawY > cssH) continue;

      atlas.ensure(
        image.hash,
        image.mime,
        pos.w,
        imageHeight,
        image.blurhash,
        image.aspectRatio,
        pos.y + pos.h / 2,
        true,
      );
      const entry = atlas.get(image.hash);

      // Clip to rounded rect so images have rounded corners
      ctx.save();
      ctx.beginPath();
      ctx.roundRect(pos.x, drawY, pos.w, imageHeight, br);
      ctx.clip();

      const isVideo = image.mime.startsWith('video/');
      const drawThumb = (thumbnailFitMode === 'contain' || isVideo) ? drawImageContain : drawImageCover;
      if (entry?.thumb) {
        const fadeEndAt = entry.blurhashFadeEndAt;
        const fadeStartAt = entry.blurhashFadeStartAt;
        const hasBlurhashCrossfade = !!entry.blurhash && fadeEndAt > now;

        if (hasBlurhashCrossfade) {
          // Keep blurhash fully opaque and only fade the thumbnail in.
          // Cross-dissolving both layers causes a perceptible global dim/fade effect.
          drawImageCover(ctx, entry.blurhash!, pos.x, drawY, pos.w, imageHeight);
          if (now >= fadeStartAt) {
            const duration = Math.max(1, fadeEndAt - fadeStartAt);
            const progress = Math.min(1, Math.max(0, (now - fadeStartAt) / duration));
            hasActiveCrossfade = progress < 1;
            ctx.globalAlpha = progress;
            drawThumb(ctx, entry.thumb, pos.x, drawY, pos.w, imageHeight);
            ctx.globalAlpha = 1;
          } else {
            hasActiveCrossfade = true;
          }
        } else {
          drawThumb(ctx, entry.thumb, pos.x, drawY, pos.w, imageHeight);
        }
      } else if (entry?.blurhash) {
        drawImageCover(ctx, entry.blurhash, pos.x, drawY, pos.w, imageHeight);
      } else {
        ctx.fillStyle = theme.placeholderBg;
        ctx.fillRect(pos.x, drawY, pos.w, imageHeight);
      }

      ctx.restore();
    }

    // Pass 2: Inner borders (batched single stroke)
    // In contain mode, border wraps the actual image rect, not the cell
    ctx.strokeStyle = theme.innerBorder;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let n = 0; n < visibleIterEnd; n++) {
      const i = visibleIndices ? visibleIndices[n] : startIdx + n;
      const pos = positions[i];
      const image = imgs[i];
      const drawY = pos.y - scrollTop;
      const imageHeight = pos.h - th;
      if (drawY + pos.h < 0 || drawY > cssH) continue;
      if (thumbnailFitMode === 'contain' && image?.aspectRatio) {
        const scale = Math.min(pos.w / image.aspectRatio, imageHeight);
        const iw = image.aspectRatio * scale;
        const ih = scale;
        const ix = pos.x + (pos.w - iw) / 2;
        const iy = drawY + (imageHeight - ih) / 2;
        ctx.roundRect(ix + 0.5, iy + 0.5, iw - 1, ih - 1, br);
      } else {
        ctx.roundRect(pos.x + 0.5, drawY + 0.5, pos.w - 1, imageHeight - 1, br);
      }
    }
    ctx.stroke();

    // Pass 3: Badges
    {
      const isContain = thumbnailFitMode === 'contain';
      for (let n = 0; n < visibleIterEnd; n++) {
        const i = visibleIndices ? visibleIndices[n] : startIdx + n;
        const pos = positions[i];
        const image = imgs[i];
        if (!image) continue;
        const drawY = pos.y - scrollTop;
        if (drawY + pos.h < 0 || drawY > cssH) continue;

        // In contain mode, compute the actual image rect so badges sit inside the visible image
        const imgH = pos.h - th;
        let bx = pos.x;
        let by = drawY;
        let bw = pos.w;
        if (isContain && !isCollectionTile(image) && image.aspectRatio) {
          const scale = Math.min(pos.w / image.aspectRatio, imgH);
          const iw = image.aspectRatio * scale;
          const ih = scale;
          bx = pos.x + (pos.w - iw) / 2;
          by = drawY + (imgH - ih) / 2;
          bw = iw;
        }

        const ext = mimeToExt(image.mime);
        const isVideo = image.mime.startsWith('video/');
        const isAnimated = image.mime === 'image/gif' && (image.num_frames ?? 0) > 1;
        const isCollection = isCollectionTile(image);
        const showBadge = !isCollection && showExtensionLabelRef.current && ext && !BADGE_HIDDEN_TYPES.has(ext.toLowerCase());

        if (showBadge) {
          drawBadge(ctx, ext.toUpperCase(), bx + 5, by + 5);
        }

        if ((isVideo || isAnimated) && typeof image.duration_ms === 'number' && image.duration_ms > 0 && videoScrubIdxRef.current !== i) {
          const durText = formatDuration(image.duration_ms);
          ctx.font = BADGE_FONT;
          const durW = ctx.measureText(durText).width + 8;
          drawBadge(ctx, durText, bx + bw - durW - 5, by + 5);
        }

        if (isCollection) {
          // "Collection" label — top-left
          drawBadge(ctx, 'Collection', bx + 5, by + 5);
          // Item count — bottom-left
          const itemCount = Math.max(0, image.collection_item_count ?? 0);
          const countText = `${itemCount.toLocaleString()} items`;
          drawBadge(ctx, countText, bx + 5, by + imgH - BADGE_H - 5);
        }
      }
    }

    // Pass 6: Text below images
    const showName = showTileNameRef.current;
    const showRes = showResolutionRef.current;
    // Keep labels visible during both scrolling and marquee drag.
    if ((showName || showRes) && th > 0) {
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      // 6a: Filenames
      if (showName) {
        ctx.font = NAME_FONT;
        ctx.fillStyle = theme.textPrimary;
        for (let n = 0; n < visibleIterEnd; n++) {
          const i = visibleIndices ? visibleIndices[n] : startIdx + n;
          const pos = positions[i];
          const image = imgs[i];
          if (!image) continue;
          if (image.hash === renamingHashRef.current) continue; // suppress for inline rename
          const drawY = pos.y - scrollTop;
          const imageHeight = pos.h - th;
          if (drawY + pos.h < 0 || drawY > cssH) continue;
          const textX = pos.x + pos.w / 2;
          const nameY = drawY + imageHeight + 14;
          const textMaxW = pos.w - 8;
          const ext = mimeToExt(image.mime);
          const nameStr = (image.name || 'Untitled') + (showExtensionRef.current && ext ? `.${ext}` : '');
          ctx.fillText(truncateText(ctx, nameStr, textMaxW), textX, nameY);
        }
      }

      // 6b: Resolution — sits below name row if name is visible, otherwise at first text row
      if (showRes) {
        ctx.font = INFO_FONT;
        ctx.fillStyle = theme.textTertiary;
        const resOffset = showName ? TEXT_NAME_ROW_H : 0;
        for (let n = 0; n < visibleIterEnd; n++) {
          const i = visibleIndices ? visibleIndices[n] : startIdx + n;
          const pos = positions[i];
          const image = imgs[i];
          if (!image || !image.width || !image.height) continue;
          const drawY = pos.y - scrollTop;
          const imageHeight = pos.h - th;
          if (drawY + pos.h < 0 || drawY > cssH) continue;
          const textX = pos.x + pos.w / 2;
          ctx.fillText(`${image.width} × ${image.height}`, textX, drawY + imageHeight + 14 + resOffset);
        }
      }
    }

    // Prefetch nearby tiles; modest during scroll to protect frame time
    const queueDepth = atlas.getQueueDepth();
    const PREFETCH_PX = isScrolling ? 900 : 3200;
    let PREFETCH_ITEM_LIMIT = isScrolling ? 28 : 420;
    if (queueDepth > 240) PREFETCH_ITEM_LIMIT = Math.min(PREFETCH_ITEM_LIMIT, 64);
    else if (queueDepth > 160) PREFETCH_ITEM_LIMIT = Math.min(PREFETCH_ITEM_LIMIT, 96);
    else if (queueDepth > 100) PREFETCH_ITEM_LIMIT = Math.min(PREFETCH_ITEM_LIMIT, 140);
    const prefetchTop = scrollTop - PREFETCH_PX;
    const prefetchBottom = scrollTop + vh + PREFETCH_PX;
    let prefetched = 0;
    if (isWaterfall) {
      const prefetchIndices = collectWaterfallIndices(
        positions,
        Math.max(0, prefetchTop),
        prefetchBottom,
        waterfallPrefetchIndicesRef.current,
      );
      for (let n = 0; n < prefetchIndices.length && prefetched < PREFETCH_ITEM_LIMIT; n++) {
        const i = prefetchIndices[n];
        const pos = positions[i];
        if (pos.y + pos.h > scrollTop && pos.y < scrollTop + vh) continue;
        const image = imgs[i];
        if (!image) continue;
        const imageHeight = pos.h - th;
        atlas.ensure(image.hash, image.mime, pos.w, imageHeight, undefined, undefined, pos.y + pos.h / 2, false);
        prefetched++;
      }
    } else {
      const [pfStart, pfEnd] = getVisibleIndices(
        positions,
        Math.max(0, prefetchTop),
        prefetchBottom - Math.max(0, prefetchTop),
        mode,
      );
      // Prefer tiles immediately after viewport
      for (let i = endIdx; i < pfEnd && i < positions.length && prefetched < PREFETCH_ITEM_LIMIT; i++) {
        if (i >= startIdx && i < endIdx) continue;
        const pos = positions[i];
        const image = imgs[i];
        if (!image) continue;
        const imageHeight = pos.h - th;
        atlas.ensure(image.hash, image.mime, pos.w, imageHeight, undefined, undefined, pos.y + pos.h / 2, false);
        prefetched++;
      }
      // Then tiles immediately before viewport
      for (let i = startIdx - 1; i >= pfStart && i >= 0 && prefetched < PREFETCH_ITEM_LIMIT; i--) {
        if (i >= startIdx && i < endIdx) continue;
        const pos = positions[i];
        const image = imgs[i];
        if (!image) continue;
        const imageHeight = pos.h - th;
        atlas.ensure(image.hash, image.mime, pos.w, imageHeight, undefined, undefined, pos.y + pos.h / 2, false);
        prefetched++;
      }
    }

    // Keep a wider cancel window than prefetch to avoid queue thrash/flicker while scrolling.
    const CANCEL_PAD_PX = isScrolling ? 1400 : 2600;
    atlas.cancelOutsideWindow(prefetchTop - CANCEL_PAD_PX, prefetchBottom + CANCEL_PAD_PX);

    // Perf instrumentation (DEV only)
    const tEnd = performance.now();
    if (GRID_DEBUG_ENABLED) {
      const perf = perfRef.current;
      const sampleElapsed = Math.max(1, tEnd - perf.sampleStart);
      perf.frames += 1;
      perf.baseFrames += 1;
      perf.drawMsTotal += tEnd - t0;
      perf.visMsTotal += tVis - t0;
      if (tEnd - t0 > 16.7) perf.slowFrames += 1;
      if (sampleElapsed >= GRID_DEBUG_SAMPLE_MS) {
        const atlasStats = atlas.getStats();
        setDebugStats({
          fps: (perf.frames * 1000) / sampleElapsed,
          drawMs: perf.drawMsTotal / perf.frames,
          visMs: perf.visMsTotal / perf.frames,
          visibleTiles: visibleIndices ? visibleIndices.length : Math.max(0, endIdx - startIdx),
          prefetchedTiles: prefetched,
          queueDepth: atlasStats.queueDepth,
          activeLoads: atlasStats.activeLoads,
          pendingBlurhash: atlasStats.pendingBlurhash,
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

    if (hasActiveCrossfade) {
      markDirty('base');
    }
  }

  // --- Overlay lane: hover, selection, marquee, reorder
  function drawOverlay() {
    const vis = lastVisibleRef.current;
    if (!vis) return;

    const overlay = overlayCanvasRef.current;
    if (!overlay) return;

    // Lazy-init overlay context
    if (!overlayCtxRef.current || overlayCtxRef.current.canvas !== overlay) {
      overlayCtxRef.current = overlay.getContext('2d', { alpha: true });
    }
    const ctx = overlayCtxRef.current;
    if (!ctx) return;

    // Resize overlay buffer to match base canvas
    const dpr = window.devicePixelRatio || 1;
    const [cssW, cssH_overlay] = ensureCanvasSize(overlay, dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH_overlay);

    const theme = themeRef.current;
    if (!theme) return;

    const positions = layoutRef.current.positions;
    const imgs = imagesRef.current;
    const selected = selectedHashesRef.current;
    const hoveredIdx = hoveredTileRef.current;
    const mRect = marqueeRectRefProp?.current ?? marqueeRectRef.current;
    const mHits = marqueeHitHashesRefProp?.current ?? marqueeHitHashesRef.current;
    const isScrolling = isScrollingRef.current;

    const { startIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br } = vis;

    // Pass 4: Hover zoom button (skip collections)
    const hoveredImg = hoveredIdx != null ? imgs[hoveredIdx] : null;
    if (hoveredIdx != null && !isScrolling && hoveredImg && !hoveredImg.is_collection) {
      const pos = positions[hoveredIdx];
      if (pos) {
        const drawY = pos.y - scrollTop;
        if (drawY + pos.h >= 0 && drawY <= cssH) {
          const imageHeight = pos.h - th;
          ctx.fillStyle = 'rgba(0, 0, 0, 0.4)';
          const bgW = ZOOM_BTN_SIZE + 4;
          const bgH = ZOOM_BTN_SIZE + 2;
          const zx = pos.x + pos.w - bgW;
          const zy = drawY + imageHeight - bgH;
          ctx.beginPath();
          ctx.roundRect(zx, zy, bgW, bgH, [10, 0, br, 0]);
          ctx.fill();
          ctx.strokeStyle = 'rgba(255, 255, 255, 0.7)';
          ctx.lineWidth = 1.5;
          const cx = zx + bgW / 2;
          const cy = zy + bgH / 2;
          ctx.beginPath();
          ctx.arc(cx, cy, 5, 0, Math.PI * 2);
          ctx.stroke();
          ctx.beginPath();
          ctx.moveTo(cx + 3.5, cy + 3.5);
          ctx.lineTo(cx + 6, cy + 6);
          ctx.stroke();
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(cx, cy - 2.5);
          ctx.lineTo(cx, cy + 2.5);
          ctx.stroke();
          ctx.beginPath();
          ctx.moveTo(cx - 2.5, cy);
          ctx.lineTo(cx + 2.5, cy);
          ctx.stroke();
        }
      }
    }

    // Pass 5: Selection outlines (batched single stroke)
    {
      ctx.strokeStyle = theme.primaryColor;
      ctx.lineWidth = 2;
      ctx.beginPath();
      let hasSelections = false;
      for (let n = 0; n < visibleIterEnd; n++) {
        const i = visibleIndices ? visibleIndices[n] : startIdx + n;
        const image = imgs[i];
        if (!image) continue;
        const isSelected = selected.has(image.hash) || (mHits?.has(image.hash) ?? false);
        if (!isSelected) continue;
        const pos = positions[i];
        const drawY = pos.y - scrollTop;
        if (drawY + pos.h < 0 || drawY > cssH) continue;
        const imgH = pos.h - th;
        // Always outline the full cell (square in grid mode), not the letterboxed image
        ctx.roundRect(pos.x - 1, drawY - 1, pos.w + 2, imgH + 2, br);
        hasSelections = true;
      }
      if (hasSelections) ctx.stroke();
    }

    // Pass 7: Marquee overlay
    if (mRect && marqueeActiveRef.current) {
      const mx = mRect.left;
      const my = mRect.top - scrollTop;
      ctx.fillStyle = 'rgba(51, 154, 240, 0.12)';
      ctx.fillRect(mx, my, mRect.width, mRect.height);
      ctx.strokeStyle = 'rgba(51, 154, 240, 0.5)';
      ctx.lineWidth = 1;
      ctx.strokeRect(mx + 0.5, my + 0.5, mRect.width - 1, mRect.height - 1);
    }

    // Pass 8: Reorder drop indicator
    const rd = reorderDragRef.current;
    if (rd?.started && rd.dropIndex != null && rd.dropSide) {
      const pos = positions[rd.dropIndex];
      if (pos) {
        const indicatorX = rd.dropSide === 'left'
          ? pos.x - gap / 2
          : pos.x + pos.w + gap / 2;
        const drawY = pos.y - scrollTop;
        const drawH = pos.h;

        // Vertical line
        ctx.strokeStyle = '#228be6';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(indicatorX, drawY);
        ctx.lineTo(indicatorX, drawY + drawH);
        ctx.stroke();

        // Small triangle at top
        ctx.fillStyle = '#228be6';
        ctx.beginPath();
        ctx.moveTo(indicatorX - 5, drawY);
        ctx.lineTo(indicatorX + 5, drawY);
        ctx.lineTo(indicatorX, drawY + 7);
        ctx.closePath();
        ctx.fill();
      }
    }

    if (GRID_DEBUG_ENABLED) perfRef.current.overlayFrames += 1;
  }

  drawBaseRef.current = drawBase;
  drawOverlayRef.current = drawOverlay;

  // --- Scroll handler
  useEffect(() => {
    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) return;

    const initialMetrics = getScrollMetrics();
    viewportHeightRef.current = initialMetrics.viewportHeight;
    scrollTopRef.current = initialMetrics.localScrollTop;
    setCanvasHeight(initialMetrics.viewportHeight);
    markDirty('both');

    let rafId = 0;
    let scrollIdleTimer = 0;

    const onScroll = () => {
      isScrollingRef.current = true;
      if (scrollIdleTimer) window.clearTimeout(scrollIdleTimer);
      scrollIdleTimer = window.setTimeout(() => {
        isScrollingRef.current = false;
        if (pendingAtlasDirtyRef.current) {
          pendingAtlasDirtyRef.current = false;
        }
        markDirty('both');
      }, 120);

      // Dismiss hover on scroll
      if (hoverTimerRef.current) {
        clearTimeout(hoverTimerRef.current);
        hoverTimerRef.current = null;
      }
      if (hoverHideTimerRef.current) {
        clearTimeout(hoverHideTimerRef.current);
        hoverHideTimerRef.current = null;
      }
      if (hoveredTileRef.current != null) {
        hoveredTileRef.current = null;
      }
      setHoverPreview((prev) => (prev ? null : prev));
      // Dismiss video scrub on scroll
      if (videoScrubTimerRef.current) {
        clearTimeout(videoScrubTimerRef.current);
        videoScrubTimerRef.current = null;
      }
      videoScrubIdxRef.current = null;
      setVideoScrub((prev) => (prev ? null : prev));

      if (rafId) return;
      rafId = requestAnimationFrame(() => {
        rafId = 0;
        const metrics = getScrollMetrics();
        scrollTopRef.current = metrics.localScrollTop;
        drawBaseRef.current();
        drawOverlayRef.current();

        // Load-more sentinel
        const onLoadMoreFn = onLoadMoreRef.current;
        if (onLoadMoreFn) {
          const st = metrics.localScrollTop;
          const vh = metrics.viewportHeight;
          const totalH = layoutRef.current.totalHeight;
          if (st + vh > totalH - LOAD_MORE_THRESHOLD) {
            onLoadMoreFn();
          }
        }
      });
    };

    const onResize = () => {
      const metrics = getScrollMetrics();
      viewportHeightRef.current = metrics.viewportHeight;
      setCanvasHeight(metrics.viewportHeight);
      markDirty('both');
    };

    scrollEl.addEventListener('scroll', onScroll, { passive: true });
    const ro = new ResizeObserver(onResize);
    ro.observe(scrollEl);

    return () => {
      scrollEl.removeEventListener('scroll', onScroll);
      ro.disconnect();
      if (rafId) cancelAnimationFrame(rafId);
      if (scrollIdleTimer) window.clearTimeout(scrollIdleTimer);
      isScrollingRef.current = false;
    };
  }, [scrollContainerRef, markDirty, getScrollMetrics]);

  useEffect(() => { markDirty('both'); }, [layout, markDirty]);

  // Scroll anchor: keep the same content centered when layout changes (zoom, view mode switch)
  useEffect(() => {
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = layout;
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
    let bestDist = Infinity;
    for (let i = 0; i < prev.positions.length; i++) {
      const p = prev.positions[i];
      const tileCenter = p.y + p.h / 2;
      const dist = Math.abs(tileCenter - viewportCenter);
      if (dist < bestDist) { bestDist = dist; anchorIdx = i; }
    }
    if (anchorIdx < 0 || anchorIdx >= layout.positions.length) return;

    const oldTileCenter = prev.positions[anchorIdx].y + prev.positions[anchorIdx].h / 2;
    const offsetInViewport = oldTileCenter - st;
    const newTileCenter = layout.positions[anchorIdx].y + layout.positions[anchorIdx].h / 2;
    const newScrollTop = newTileCenter - offsetInViewport;
    scrollEl.scrollTop = Math.max(0, metrics.canvasTopInScroll + newScrollTop);
  }, [layout, getScrollMetrics, scrollContainerRef]);

  useEffect(() => { markDirty('overlay'); }, [selectedHashes, markDirty]);
  useEffect(() => { markDirty('base'); }, [thumbnailFitMode, showExtension, showExtensionLabel, markDirty]);

  useEffect(() => {
    return () => {
      if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
      if (hoverHideTimerRef.current) clearTimeout(hoverHideTimerRef.current);
      if (unfreezeSettleTimerRef.current) clearTimeout(unfreezeSettleTimerRef.current);
    };
  }, []);

  // --- Hit testing
  function hitTest(clientX: number, clientY: number): number | null {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    const mx = clientX - rect.left;
    const my = clientY - rect.top + scrollTopRef.current;

    const positions = layoutRef.current.positions;
    const mode = viewModeRef.current;
    if (mode === 'waterfall') {
      const candidates = collectWaterfallIndices(
        positions,
        scrollTopRef.current,
        scrollTopRef.current + viewportHeightRef.current,
        waterfallHitIndicesRef.current,
      );
      for (let n = 0; n < candidates.length; n++) {
        const i = candidates[n];
        const pos = positions[i];
        if (mx >= pos.x && mx < pos.x + pos.w && my >= pos.y && my < pos.y + pos.h) {
          return i;
        }
      }
      return null;
    }

    const [startIdx, endIdx] = getVisibleIndices(
      positions, scrollTopRef.current, viewportHeightRef.current, mode,
    );

    for (let i = startIdx; i < endIdx && i < positions.length; i++) {
      const pos = positions[i];
      if (mx >= pos.x && mx < pos.x + pos.w && my >= pos.y && my < pos.y + pos.h) {
        return i;
      }
    }
    return null;
  }

  function isZoomButtonHit(clientX: number, clientY: number, tileIdx: number): boolean {
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
  }

  // --- Reorder drop target
  function computeReorderTarget(clientX: number, clientY: number, draggedSet: Set<string>): { index: number; side: 'left' | 'right' } | null {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    const mx = clientX - rect.left;
    // Read scroll position fresh from the DOM — scrollTopRef can be stale during
    // auto-scroll because the scroll handler updates it asynchronously via RAF.
    const scrollTop = getScrollMetrics().localScrollTop;
    const my = clientY - rect.top + scrollTop;
    const positions = layoutRef.current.positions;

    // Check each visible tile
    const mode = viewModeRef.current;
    if (mode === 'waterfall') {
      const candidates = collectWaterfallIndices(
        positions,
        scrollTop,
        scrollTop + viewportHeightRef.current,
        waterfallHitIndicesRef.current,
      );
      for (let n = 0; n < candidates.length; n++) {
        const i = candidates[n];
        const pos = positions[i];
        if (mx >= pos.x && mx < pos.x + pos.w && my >= pos.y && my < pos.y + pos.h) {
          const img = imagesRef.current[i];
          if (img && draggedSet.has(img.hash)) return null;
          const midX = pos.x + pos.w / 2;
          return { index: i, side: mx < midX ? 'left' : 'right' };
        }
      }
    } else {
      const [startIdx, endIdx] = getVisibleIndices(
        positions, scrollTop, viewportHeightRef.current, mode,
      );
      for (let i = startIdx; i < endIdx && i < positions.length; i++) {
        const pos = positions[i];
        if (mx >= pos.x && mx < pos.x + pos.w && my >= pos.y && my < pos.y + pos.h) {
          // Skip tiles that are being dragged
          const img = imagesRef.current[i];
          if (img && draggedSet.has(img.hash)) return null;
          const midX = pos.x + pos.w / 2;
          return { index: i, side: mx < midX ? 'left' : 'right' };
        }
      }
    }

    // Check if past last tile on last row — allow drop at end
    if (positions.length > 0) {
      const lastPos = positions[positions.length - 1];
      if (my >= lastPos.y && my <= lastPos.y + lastPos.h && mx > lastPos.x + lastPos.w) {
        return { index: positions.length - 1, side: 'right' };
      }
    }

    return null;
  }

  // --- Event handlers
  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0 || !e.isPrimary) return;
    const idx = hitTest(e.clientX, e.clientY);
    if (idx == null) return;
    const image = imagesRef.current[idx];
    if (!image) return;

    // Prevent parent scroll container from starting marquee selection
    e.stopPropagation();

    // Check zoom button
    if (isZoomButtonHit(e.clientX, e.clientY, idx)) return;

    // Scope-level drag guard
    if (dragDisabledRef.current) return;

    const state = { hash: image.hash, startX: e.clientX, startY: e.clientY, started: false };
    dragStateRef.current = state;

    const sel = imageDrag.getSelectedHashes();
    const isSelected = selectedHashesRef.current.has(image.hash);
    const hashes = isSelected && sel.size > 0 ? Array.from(sel) : [image.hash];

    // Unified drag: on threshold, hand off to OS native drag.
    // HTML5 drag events (dragover/drop) handle reorder + sidebar folder drops.
    const handleMove = (me: PointerEvent) => {
      const dx = me.clientX - state.startX;
      const dy = me.clientY - state.startY;
      if (!state.started && dx * dx + dy * dy > DRAG_THRESHOLD_SQ) {
        state.started = true;
        cleanup(); // Remove pointer listeners — OS takes over from here

        // Store dragged set for reorder drop target computation
        draggedHashSetRef.current = new Set(hashes);

        // If reorder mode, initialize reorder drag state for canvas indicator
        if (reorderModeRef.current) {
          reorderDragRef.current = {
            draggedHashes: hashes,
            startX: state.startX,
            startY: state.startY,
            started: true,
            dropIndex: null,
            dropSide: null,
          };
        }

        // Start native drag session — marks hashes as pending so
        // sidebar/window handlers recognize this as an internal drag
        const sessionId = imageDrag.startNativeDragSession(hashes);

        // Generate drag icon (thumbnail + count badge) then start OS drag.
        // The thumbnail is likely browser-cached so the Image load is near-instant.
        const startDrag = (iconDataUrl?: string | null) => {
          getCurrentWebview().startNativeDrag(hashes, iconDataUrl)
            .catch(() => {
              imageDrag.clearNativeDragSession(sessionId);
            });
        };

        const thumbUrl = mediaThumbnailUrl(image.hash);
        const iconImg = new Image();
        iconImg.onload = () => {
          startDrag(createDragIcon(iconImg, hashes.length));
        };
        iconImg.onerror = () => startDrag();
        iconImg.src = thumbUrl;
      }
    };

    const cleanup = () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };

    const handleUp = () => {
      cleanup();
      // If we never started dragging, it's just a click — handleClick processes it
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  }, []);

  // --- Canvas HTML5 drag event handlers (internal drag / reorder)
  const EDGE_ZONE = 60; // px from edge where auto-scroll activates
  const MAX_SCROLL_SPEED = 12; // px/frame at the very edge

  const handleCanvasDragOver = useCallback((e: React.DragEvent) => {
    const draggedSet = draggedHashSetRef.current;
    if (!draggedSet) return; // External drop — let window handler show import overlay

    // Internal drag — always claim the event so it never reaches the
    // window import handler (no overlay, no import).
    e.preventDefault();
    e.stopPropagation();

    // In reorder mode, also compute the drop target indicator
    if (reorderModeRef.current) {
      e.dataTransfer.dropEffect = 'move';
      const target = computeReorderTarget(e.clientX, e.clientY, draggedSet);
      const rd = reorderDragRef.current;
      if (!rd) return;
      const newIdx = target?.index ?? null;
      const newSide = target?.side ?? null;
      if (newIdx !== rd.dropIndex || newSide !== rd.dropSide) {
        rd.dropIndex = newIdx;
        rd.dropSide = newSide;
        markDirty('overlay');
      }

      // Auto-scroll when cursor near top/bottom edge
      const scrollEl = scrollContainerRef?.current;
      if (scrollEl) {
        const rect = scrollEl.getBoundingClientRect();
        const cursorY = e.clientY;
        const distFromTop = cursorY - rect.top;
        const distFromBottom = rect.bottom - cursorY;
        const as = autoScrollRef.current;

        // Arm once cursor has been in the safe interior (not in edge zone)
        if (distFromTop > EDGE_ZONE && distFromBottom > EDGE_ZONE) {
          as.armed = true;
        }

        if (as.armed && distFromTop < EDGE_ZONE) {
          // Near top — scroll up (negative speed)
          const ratio = 1 - distFromTop / EDGE_ZONE;
          as.speed = -Math.round(MAX_SCROLL_SPEED * ratio);
          startAutoScroll();
        } else if (as.armed && distFromBottom < EDGE_ZONE) {
          // Near bottom — scroll down (positive speed)
          const ratio = 1 - distFromBottom / EDGE_ZONE;
          as.speed = Math.round(MAX_SCROLL_SPEED * ratio);
          startAutoScroll();
        } else {
          stopAutoScroll();
        }
      }
    }
  }, [markDirty, scrollContainerRef, startAutoScroll, stopAutoScroll]);

  const handleCanvasDrop = useCallback((e: React.DragEvent) => {
    const draggedSet = draggedHashSetRef.current;
    if (!draggedSet) return; // External drop — let window handler import

    // Internal drag — block import, execute reorder if applicable
    e.preventDefault();
    e.stopPropagation();
    stopAutoScroll();
    autoScrollRef.current.armed = false;

    if (reorderModeRef.current) {
      // Re-compute target at drop time — the cached values from the last dragOver
      // may be stale if scroll position changed (especially during auto-scroll).
      const target = computeReorderTarget(e.clientX, e.clientY, draggedSet);
      const rd = reorderDragRef.current;
      if (target && rd) {
        const targetIndex = target.side === 'right' ? target.index + 1 : target.index;
        onReorderRef.current?.(rd.draggedHashes, targetIndex);
      }
    }

    imageDrag.clearNativeDragSession();
    reorderDragRef.current = null;
    draggedHashSetRef.current = null;
    markDirty('overlay');
  }, [markDirty, stopAutoScroll]);

  const handleCanvasDragLeave = useCallback(() => {
    stopAutoScroll();
    const rd = reorderDragRef.current;
    if (rd) {
      rd.dropIndex = null;
      rd.dropSide = null;
      markDirty('overlay');
    }
  }, [markDirty, stopAutoScroll]);

  const handleClick = useCallback((e: React.MouseEvent) => {
    if (dragStateRef.current?.started) {
      dragStateRef.current = null;
      return;
    }
    dragStateRef.current = null;

    const idx = hitTest(e.clientX, e.clientY);
    if (idx == null) return;
    const image = imagesRef.current[idx];
    if (!image) return;

    // Check zoom button — trigger hover preview instead (skip videos and collections)
    if (isZoomButtonHit(e.clientX, e.clientY, idx)) {
      if (!isVideoMime(image.mime) && !image.is_collection) {
        if (hoverHideTimerRef.current) {
          clearTimeout(hoverHideTimerRef.current);
          hoverHideTimerRef.current = null;
        }
        setHoverPreview((prev) => (
          prev && prev.hash === image.hash && prev.mime === image.mime
            ? prev
            : { hash: image.hash, mime: image.mime }
        ));
      }
      return;
    }

    onImageClickRef.current(image, e);
  }, []);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (marqueeActiveRef.current) return;

    const idx = hitTest(e.clientX, e.clientY);
    const prevIdx = hoveredTileRef.current;

    if (idx !== prevIdx) {
      hoveredTileRef.current = idx;
      markDirty('overlay');
    }

    // Hover preview on zoom button area (skip videos and collections)
    if (idx != null && isZoomButtonHit(e.clientX, e.clientY, idx)) {
      const image = imagesRef.current[idx];
      const isPreviewable = image && !isVideoMime(image.mime) && !image.is_collection;
      if (hoverHideTimerRef.current) {
        clearTimeout(hoverHideTimerRef.current);
        hoverHideTimerRef.current = null;
      }
      if (isPreviewable && !hoverTimerRef.current) {
        hoverTimerRef.current = setTimeout(() => {
          setHoverPreview((prev) => (
            prev && prev.hash === image.hash && prev.mime === image.mime
              ? prev
              : { hash: image.hash, mime: image.mime }
          ));
          hoverTimerRef.current = null;
        }, PREVIEW_DELAY_MS);
      }
    } else {
      if (hoverTimerRef.current) {
        clearTimeout(hoverTimerRef.current);
        hoverTimerRef.current = null;
      }
      if (!hoverHideTimerRef.current) {
        hoverHideTimerRef.current = setTimeout(() => {
          setHoverPreview(null);
          hoverHideTimerRef.current = null;
        }, 90);
      }
    }

    // Video scrub overlay — 500ms delay on video tiles
    if (idx !== videoScrubIdxRef.current) {
      // Tile changed — clear any pending timer
      if (videoScrubTimerRef.current) {
        clearTimeout(videoScrubTimerRef.current);
        videoScrubTimerRef.current = null;
      }
      videoScrubIdxRef.current = idx;
      // Dismiss active scrub if tile changed
      setVideoScrub(null);

      if (idx != null) {
        const image = imagesRef.current[idx];
        if (image && isVideoMime(image.mime) && image.duration_ms && image.duration_ms > 0) {
          videoScrubTimerRef.current = setTimeout(() => {
            videoScrubTimerRef.current = null;
            const canvas = canvasRef.current;
            if (!canvas) return;
            const canvasRect = canvas.getBoundingClientRect();
            const pos = layoutRef.current.positions[idx];
            if (!pos) return;
            const th = textHeightRef.current;
            const imageH = pos.h - th;
            const rect: VideoScrubRect = {
              left: canvasRect.left + pos.x,
              top: canvasRect.top + pos.y - scrollTopRef.current,
              width: pos.w,
              height: imageH,
            };
            setVideoScrub({
              index: idx,
              hash: image.hash,
              mime: image.mime,
              durationSec: image.duration_ms! / 1000,
              rect,
            });
          }, VIDEO_SCRUB_DELAY_MS);
        }
      }
    }
  }, [markDirty]);

  const handleMouseLeave = useCallback(() => {
    if (hoveredTileRef.current != null) {
      hoveredTileRef.current = null;
      markDirty('overlay');
    }
    if (hoverTimerRef.current) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
    if (hoverHideTimerRef.current) {
      clearTimeout(hoverHideTimerRef.current);
      hoverHideTimerRef.current = null;
    }
    setHoverPreview(null);
    // Clear pending video scrub timer (but don't dismiss active overlay —
    // the overlay portal sits above the canvas, so mouse-leave fires when
    // entering the overlay; the overlay's own onMouseLeave handles dismiss)
    if (videoScrubTimerRef.current) {
      clearTimeout(videoScrubTimerRef.current);
      videoScrubTimerRef.current = null;
    }
  }, [markDirty]);

  // --- Pop animation
  useEffect(() => {
    if (!popHash) return;
    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) { onPopComplete?.(); return; }

    const positions = layoutRef.current.positions;
    const imgs = imagesRef.current;
    const idx = imgs.findIndex(img => img.hash === popHash);
    if (idx === -1 || !positions[idx]) { onPopComplete?.(); return; }

    const pos = positions[idx];
    const metrics = getScrollMetrics();
    const viewportH = metrics.viewportHeight;
    const scrollTop = metrics.localScrollTop;

    // Scroll into view if needed
    if (pos.y < scrollTop || pos.y + pos.h > scrollTop + viewportH) {
      const targetLocalScroll = pos.y - viewportH / 2 + pos.h / 2;
      scrollEl.scrollTop = Math.max(0, metrics.canvasTopInScroll + targetLocalScroll);
    }
    onPopComplete?.();
  }, [popHash, scrollContainerRef, onPopComplete, getScrollMetrics]);

  // Not yet measured
  if (containerWidth === 0) {
    return <div ref={containerRef} style={{ minHeight: 1 }} />;
  }

  // Empty state — drop area
  if (renderImages.length === 0) {
    if (!showEmptyState) {
      return <div ref={containerRef} style={{ minHeight: 1 }} />;
    }
    const hasSearchTags = !!searchTags?.length;
    const title = getEmptyStateTitle(emptyContext, hasSearchTags);
    const description = getEmptyStateDescription(emptyContext, hasSearchTags);
    const showImportActions =
      emptyContext !== 'inbox' &&
      emptyContext !== 'untagged' &&
      emptyContext !== 'smart-folder' &&
      !hasSearchTags;

    const iconNode = (
      <div
        style={{
          position: 'relative',
          width: 90,
          height: 120,
          marginBottom: -40,
          maskImage: 'linear-gradient(to bottom, black 30%, transparent 100%)',
          WebkitMaskImage: 'linear-gradient(to bottom, black 30%, transparent 100%)',
        }}
      >
        <div
          style={{
            position: 'absolute',
            inset: 0,
            borderRadius: 4,
            border: '1px solid var(--color-border-secondary)',
            background: 'linear-gradient(180deg, var(--color-border-primary) 0%, var(--color-border-secondary) 100%)',
            paddingTop: 8,
            paddingLeft: 6,
            paddingRight: 6,
            paddingBottom: 6,
          }}
        >
          <div
            style={{
              width: '100%',
              height: '100%',
              borderRadius: 2,
              border: '1px solid var(--color-border-secondary)',
              background: 'var(--color-theme)',
            }}
          />
        </div>
        <div
          style={{
            position: 'absolute',
            inset: 0,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            zIndex: 1,
          }}
        >
          <IconPhoto size={28} stroke={1.2} style={{ color: 'var(--color-text-tertiary)' }} />
        </div>
      </div>
    );

    return (
      <div ref={containerRef} style={{ position: 'relative', minHeight: 400 }}>
        <div style={{
          position: 'absolute',
          inset: 0,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          textAlign: 'center',
          padding: 40,
          boxSizing: 'border-box',
          WebkitFontSmoothing: 'antialiased',
        }}>
          <StateBlock
            variant="empty"
            iconNode={iconNode}
            title={title}
            description={description}
            action={showImportActions ? (
              <StateActions>
                <TextButton onClick={onImport}>
                  <IconUpload size={14} />
                  Import Files
                </TextButton>
                {emptyContext === 'folder' && onImportFolder && (
                  <TextButton onClick={onImportFolder}>
                    <IconFolderPlus size={14} />
                    Import Folder
                  </TextButton>
                )}
              </StateActions>
            ) : null}
          />
        </div>
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
      {GRID_DEBUG_ENABLED && debugStats && (
        <div
          style={{
            position: 'fixed',
            right: 12,
            bottom: 12,
            zIndex: 200100,
            pointerEvents: 'none',
            background: 'var(--color-black-70)',
            color: 'var(--color-text-primary)',
            border: '1px solid var(--color-white-20)',
            borderRadius: 8,
            padding: '8px 10px',
            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
            fontSize: 'var(--font-size-2xs)',
            lineHeight: 'var(--line-height-normal)',
            minWidth: 240,
            whiteSpace: 'pre',
          }}
        >
{`fps ${debugStats.fps.toFixed(1)}  draw ${debugStats.drawMs.toFixed(2)}ms  vis ${debugStats.visMs.toFixed(2)}ms
tiles vis ${debugStats.visibleTiles}  prefetch ${debugStats.prefetchedTiles}
atlas q ${debugStats.queueDepth}  active ${debugStats.activeLoads}  blur ${debugStats.pendingBlurhash}
cache ${debugStats.cacheSize}  slowFrames ${debugStats.slowFrames}  disk ${debugStats.diskSpeed}
base ${debugStats.baseRedraws}  overlay ${debugStats.overlayRedraws}`}
        </div>
      )}
      {hoverPreview && <HoverPreview {...hoverPreview} />}
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
