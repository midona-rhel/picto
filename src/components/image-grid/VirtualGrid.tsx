import {
  useRef,
  useState,
  useEffect,
  useCallback,
  useMemo,
  memo,
  RefObject,
} from 'react';
import { createPortal } from 'react-dom';
import { IconPhoto, IconUpload } from '@tabler/icons-react';
import { EmptyState } from '../ui/EmptyState';
import { TextButton } from '../ui/TextButton';
import { decode } from 'blurhash';
import { MasonryImageItem, isVideoMime } from './shared';
import { VideoScrubOverlay, type VideoScrubRect } from './VideoScrubOverlay';
import { mediaThumbnailUrl, mediaFileUrl } from '../../lib/mediaUrl';
import { formatDuration } from '../../lib/formatters';
import { imageDrag } from '../../lib/imageDrag';
import type { GridViewMode } from './runtime';
import styles from './VirtualGrid.module.css';

const THUMB_MAX_SIDE = 900;
const DRAG_THRESHOLD_SQ = 25; // 5px²
const OVERSCAN_PX = 5000;
export const TEXT_NAME_ROW_H = 20;
export const TEXT_RESOLUTION_ROW_H = 20;

/** Compute total text area height based on which fields are visible. */
export function computeTextHeight(showName: boolean, showResolution: boolean): number {
  let h = 0;
  if (showName) h += TEXT_NAME_ROW_H;
  if (showResolution) h += TEXT_RESOLUTION_ROW_H;
  return h;
}

export const TEXT_AREA_H = TEXT_NAME_ROW_H + TEXT_RESOLUTION_ROW_H;

const BADGE_HIDDEN_TYPES = new Set(['jpg', 'jpeg', 'png', 'webp']);

function blurhashToDataUrl(hash: string, aspectRatio: number): string {
  try {
    let w: number, h: number;
    if (aspectRatio >= 1) {
      w = 32;
      h = Math.max(1, Math.round(32 / aspectRatio));
    } else {
      h = 32;
      w = Math.max(1, Math.round(32 * aspectRatio));
    }
    const pixels = decode(hash, w, h);
    const canvas = document.createElement('canvas');
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext('2d');
    if (!ctx) return '';
    const imageData = ctx.createImageData(w, h);
    imageData.data.set(pixels);
    ctx.putImageData(imageData, 0, 0);
    return canvas.toDataURL();
  } catch {
    return '';
  }
}

const blurhashCache = new Map<string, string>();
function getCachedBlurhash(hash: string | null | undefined, aspectRatio: number): string {
  if (!hash) return '';
  const key = `${hash}:${aspectRatio.toFixed(2)}`;
  const cached = blurhashCache.get(key);
  if (cached !== undefined) return cached;
  const url = blurhashToDataUrl(hash, aspectRatio);
  blurhashCache.set(key, url);
  return url;
}

export interface LayoutItem {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface LayoutResult {
  positions: LayoutItem[];
  totalHeight: number;
}

function safeAspectRatio(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 1.5;
  return Math.min(8, Math.max(0.125, value));
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

export function computeLayout(
  images: MasonryImageItem[],
  containerWidth: number,
  targetSize: number,
  gap: number,
  viewMode: GridViewMode,
  textHeight: number,
  paddingX = 0,
): LayoutResult {
  if (images.length === 0 || containerWidth <= 0) {
    return { positions: [], totalHeight: 0 };
  }

  const innerWidth = containerWidth - 2 * paddingX;
  const columnCount = Math.max(1, Math.round((innerWidth + gap) / (targetSize + gap)));
  const colWidth = Math.floor((innerWidth - (columnCount - 1) * gap) / columnCount);

  let result: LayoutResult;
  if (viewMode === 'grid') {
    result = layoutGrid(images, colWidth, columnCount, gap, textHeight);
  } else if (viewMode === 'justified') {
    result = layoutJustified(images, innerWidth, targetSize, gap, textHeight);
  } else {
    result = layoutWaterfall(images, colWidth, columnCount, gap, textHeight);
  }

  // Offset positions by padding so outlines/indicators aren't clipped by canvas edges
  const paddingY = 2;
  for (const pos of result.positions) {
    if (paddingX > 0) pos.x += paddingX;
    pos.y += paddingY;
  }
  result.totalHeight += paddingY * 2;

  return result;
}

function layoutWaterfall(
  images: MasonryImageItem[],
  colWidth: number,
  columnCount: number,
  gap: number,
  textHeight: number,
): LayoutResult {
  const colHeights = new Float64Array(columnCount);
  const positions: LayoutItem[] = new Array(images.length);

  for (let i = 0; i < images.length; i++) {
    // Find shortest column
    let shortest = 0;
    for (let c = 1; c < columnCount; c++) {
      if (colHeights[c] < colHeights[shortest]) shortest = c;
    }

    const x = shortest * (colWidth + gap);
    const y = colHeights[shortest];
    const h = colWidth / safeAspectRatio(images[i].aspectRatio) + textHeight;

    positions[i] = { x, y, w: colWidth, h };
    colHeights[shortest] = y + h + gap;
  }

  let maxHeight = 0;
  for (let c = 0; c < columnCount; c++) {
    if (colHeights[c] > maxHeight) maxHeight = colHeights[c];
  }

  return { positions, totalHeight: Math.max(0, maxHeight - gap) };
}

function layoutGrid(
  images: MasonryImageItem[],
  colWidth: number,
  columnCount: number,
  gap: number,
  textHeight: number,
): LayoutResult {
  const positions: LayoutItem[] = new Array(images.length);
  const tileSize = colWidth; // square image portion
  const cellH = tileSize + textHeight;

  for (let i = 0; i < images.length; i++) {
    const col = i % columnCount;
    const row = Math.floor(i / columnCount);
    positions[i] = {
      x: col * (tileSize + gap),
      y: row * (cellH + gap),
      w: tileSize,
      h: cellH,
    };
  }

  const rows = Math.ceil(images.length / columnCount);
  const totalHeight = rows > 0 ? rows * cellH + (rows - 1) * gap : 0;
  return { positions, totalHeight };
}

function layoutJustified(
  images: MasonryImageItem[],
  containerWidth: number,
  targetRowHeight: number,
  gap: number,
  textHeight: number,
): LayoutResult {
  const positions: LayoutItem[] = new Array(images.length);
  let y = 0;
  let rowStart = 0;

  while (rowStart < images.length) {
    // Accumulate images until their natural widths at targetRowHeight exceed containerWidth
    let rowEnd = rowStart;
    let totalAspect = 0;

    while (rowEnd < images.length) {
      totalAspect += safeAspectRatio(images[rowEnd].aspectRatio);
      rowEnd++;
      // Check if this row is full
      const rowWidth = totalAspect * targetRowHeight + (rowEnd - rowStart - 1) * gap;
      if (rowWidth >= containerWidth) break;
    }

    // Compute actual row height to fit containerWidth exactly
    const count = rowEnd - rowStart;
    const gapSpace = (count - 1) * gap;
    const rowHeight = (containerWidth - gapSpace) / totalAspect;
    // Clamp: don't let last (incomplete) row be taller than 1.5× target
    const finalHeight = Math.min(rowHeight, targetRowHeight * 1.5);
    const cellH = finalHeight + textHeight;

    let x = 0;
    for (let i = rowStart; i < rowEnd; i++) {
      const w = finalHeight * safeAspectRatio(images[i].aspectRatio);
      positions[i] = { x, y, w, h: cellH };
      x += w + gap;
    }

    y += cellH + gap;
    rowStart = rowEnd;
  }

  return { positions, totalHeight: Math.max(0, y - gap) };
}

// Ignore width changes smaller than scrollbar width to prevent re-layout jitter
const SCROLLBAR_JITTER_PX = 20;

function useContainerWidth() {
  const [width, setWidth] = useState(0);
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
          setWidth(rounded);
          return;
        }
        if (timerRef.current) clearTimeout(timerRef.current);
        timerRef.current = setTimeout(() => {
          setWidth((prev) => {
            if (rounded === prev) return prev;
            // Skip small changes caused by scrollbar appearing/disappearing
            if (Math.abs(rounded - prev) <= SCROLLBAR_JITTER_PX) return prev;
            return rounded;
          });
        }, 100);
      });
      ro.observe(el);
      roRef.current = ro;
    }
  }, []);
  return { ref, width };
}

// Persist readiness across virtualized remounts to avoid fade flicker while scrolling
const thumbReadyCache = new Set<string>();
// Survives React re-renders so remounted tiles keep their src
const ioActivatedCache = new Set<string>();

const MAX_CONCURRENT_LOADS = 6;
let activeLoadCount = 0;
const loadQueue: Array<{ img: HTMLImageElement; src: string }> = [];

function scheduleLoad(img: HTMLImageElement, src: string) {
  if (activeLoadCount >= MAX_CONCURRENT_LOADS) {
    loadQueue.push({ img, src });
    return;
  }
  activeLoadCount++;
  ioActivatedCache.add(src);
  img.src = src;
}

function onLoadSlotFree() {
  activeLoadCount = Math.max(0, activeLoadCount - 1);
  if (loadQueue.length > 0) {
    const next = loadQueue.shift()!;
    activeLoadCount++;
    ioActivatedCache.add(next.src);
    next.img.src = next.src;
  }
}

function purgeLoadQueue() {
  for (let i = loadQueue.length - 1; i >= 0; i--) {
    if (!loadQueue[i].img.isConnected) {
      loadQueue.splice(i, 1);
    }
  }
}

const onThumbLoad = (e: React.SyntheticEvent<HTMLImageElement>) => {
  const img = e.target as HTMLImageElement;
  thumbReadyCache.add(img.src);
  onLoadSlotFree();
  // Wrap visual reveal in rAF to avoid synchronous style recalc during load event
  requestAnimationFrame(() => {
    img.style.opacity = '1';
    const prev = img.previousElementSibling as HTMLElement | null;
    if (prev) prev.style.opacity = '0';
  });
};

interface TileProps {
  image: MasonryImageItem;
  layout: LayoutItem;
  isSelected: boolean;
  textHeight: number;
  showTileName?: boolean;
  showResolution?: boolean;
  showExtension?: boolean;
  showExtensionLabel?: boolean;
  thumbnailFitMode?: 'cover' | 'contain';
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

const VirtualTile = memo(function VirtualTile({
  image,
  layout,
  isSelected,
  textHeight,
  showTileName = true,
  showResolution = true,
  showExtension = true,
  showExtensionLabel = true,
  thumbnailFitMode = 'cover',
}: TileProps) {
  const blurhashUrl = getCachedBlurhash(image.blurhash, image.aspectRatio);
  const imgUrl = layout.w > THUMB_MAX_SIDE
    ? mediaFileUrl(image.hash, image.mime)
    : mediaThumbnailUrl(image.hash);

  const ext = mimeToExt(image.mime);
  const isVideo = image.mime.startsWith('video/');
  const isAnimated = image.mime === 'image/gif' && (image.num_frames ?? 0) > 1;
  const durationMs = image.duration_ms;
  const showBadge = showExtensionLabel && ext && !BADGE_HIDDEN_TYPES.has(ext.toLowerCase());

  const imageHeight = layout.h - textHeight;

  const fullyLoaded = thumbReadyCache.has(imgUrl);
  const hasSrc = fullyLoaded || ioActivatedCache.has(imgUrl);

  return (
    <div
      data-hash={image.hash}
      data-mime={image.mime}
      className={styles.tileWrapper}
      style={{
        position: 'absolute',
        transform: `translate3d(${layout.x}px, ${layout.y}px, 0)`,
        width: layout.w,
        height: layout.h,
      }}
    >
      <div className={`${styles.tile} ${isSelected ? styles.tileSelected : ''} ${thumbnailFitMode === 'contain' ? styles.tileContain : ''}`} style={{ width: '100%', height: imageHeight }}>
        {blurhashUrl && (
          <img
            src={blurhashUrl}
            alt=""
            draggable={false}
            className={styles.blurhashLayer}
            style={fullyLoaded ? { opacity: 0 } : undefined}
          />
        )}
        <img
          data-src={imgUrl}
          src={hasSrc ? imgUrl : undefined}
          alt=""
          draggable={false}
          decoding="async"
          onLoad={onThumbLoad}
          className={styles.thumbLayer}
          style={fullyLoaded
            ? thumbnailFitMode === 'contain' ? { opacity: 1, objectFit: 'contain' } : { opacity: 1 }
            : thumbnailFitMode === 'contain' ? { objectFit: 'contain' } : undefined}
        />
        {showBadge && <span className={styles.extensionBadge}>{ext}</span>}
        {(isVideo || isAnimated) && typeof durationMs === 'number' && durationMs > 0 && (
          <span className={styles.durationBadge}>{formatDuration(durationMs)}</span>
        )}
        {!image.is_collection && <span className={styles.zoomBtn} />}
      </div>
      {showTileName && (
        <div className={styles.tileName} title={image.name || ''}>
          {image.name || 'Untitled'}{showExtension && ext ? `.${ext}` : ''}
        </div>
      )}
      {showTileName && showResolution && image.width && image.height && (
        <div className={styles.tileInfo}>
          {image.width} × {image.height}
        </div>
      )}
    </div>
  );
}, (prev, next) =>
  prev.image === next.image && prev.layout === next.layout &&
  prev.isSelected === next.isSelected &&
  prev.textHeight === next.textHeight &&
  prev.showTileName === next.showTileName && prev.showResolution === next.showResolution &&
  prev.showExtension === next.showExtension && prev.showExtensionLabel === next.showExtensionLabel &&
  prev.thumbnailFitMode === next.thumbnailFitMode
);

interface HoverPreviewData {
  hash: string;
  mime: string;
}

const PREVIEW_DELAY_MS = 200;
const VIDEO_SCRUB_DELAY_MS = 500;
const PREVIEW_INSET = 48;

function HoverPreview({ hash, mime }: HoverPreviewData) {
  const fullUrl = mediaFileUrl(hash, mime);
  const [loaded, setLoaded] = useState(false);

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
        onLoad={() => setLoaded(true)}
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

interface VirtualGridProps {
  images: MasonryImageItem[];
  targetSize: number;
  gap: number;
  viewMode: GridViewMode;
  selectedHashes: Set<string>;
  searchTags?: string[];
  onImageClick: (image: MasonryImageItem, event: React.MouseEvent) => void;
  onImport: () => void;
  onContainerWidthChange?: (width: number) => void;
  showEmptyState?: boolean;
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
  thumbnailFitMode?: 'cover' | 'contain';
}

export function VirtualGrid({
  images,
  targetSize,
  gap,
  viewMode,
  selectedHashes,
  searchTags,
  onImageClick,
  onImport,
  onContainerWidthChange,
  showEmptyState = true,
  onLoadMore,
  scrollContainerRef,
  popHash,
  onPopComplete,
  frozen: _frozen = false,
  marqueeActive = false,
  showTileName = true,
  showResolution = true,
  showExtension = true,
  showExtensionLabel = true,
  thumbnailFitMode = 'cover',
}: VirtualGridProps) {
  const { ref: containerRef, width: containerWidth } = useContainerWidth();

  // Scroll state in refs to avoid re-renders per scroll frame
  const scrollTopRef = useRef(0);
  const viewportHeightRef = useRef(0);
  const isScrollingRef = useRef(false);
  const perfContainerRef = useRef<HTMLDivElement>(null);
  const prevRangeKeyRef = useRef('');

  const dragStateRef = useRef<{ hash: string; startX: number; startY: number; started: boolean } | null>(null);

  const [visibleIndices, setVisibleIndices] = useState<number[]>([]);

  const [hoverPreview, setHoverPreview] = useState<HoverPreviewData | null>(null);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [videoScrub, setVideoScrub] = useState<{
    hash: string;
    mime: string;
    durationSec: number;
    rect: VideoScrubRect;
  } | null>(null);
  const videoScrubTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const videoScrubHashRef = useRef<string | null>(null);

  useEffect(() => {
    if (!_frozen) return;
    if (videoScrubTimerRef.current) {
      clearTimeout(videoScrubTimerRef.current);
      videoScrubTimerRef.current = null;
    }
    videoScrubHashRef.current = null;
    setVideoScrub((prev) => (prev ? null : prev));
  }, [_frozen]);

  const handleHoverPreview = useCallback((hash: string, mime: string) => {
    if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
    hoverTimerRef.current = setTimeout(() => {
      setHoverPreview({ hash, mime });
    }, PREVIEW_DELAY_MS);
  }, []);

  const handleHoverPreviewHide = useCallback(() => {
    if (hoverTimerRef.current) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
    setHoverPreview(null);
  }, []);

  const imagesByHash = useMemo(() => {
    const map = new Map<string, MasonryImageItem>();
    for (const img of images) map.set(img.hash, img);
    return map;
  }, [images]);

  const selectedHashesRef = useRef(selectedHashes);
  selectedHashesRef.current = selectedHashes;
  const onImageClickRef = useRef(onImageClick);
  onImageClickRef.current = onImageClick;
  const imagesByHashRef = useRef(imagesByHash);
  imagesByHashRef.current = imagesByHash;

  // --- Delegated handlers
  const handleGridMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const tileEl = (e.target as HTMLElement).closest(`.${styles.tile}`) as HTMLElement | null;
    if (!tileEl) return;
    const wrapperEl = tileEl.closest('[data-hash]') as HTMLElement | null;
    if (!wrapperEl) return;
    const hash = wrapperEl.dataset.hash!;

    const state = { hash, startX: e.clientX, startY: e.clientY, started: false };
    dragStateRef.current = state;

    const sel = imageDrag.getSelectedHashes();
    const isSelected = selectedHashesRef.current.has(hash);
    const hashes = isSelected && sel.size > 0 ? Array.from(sel) : [hash];

    const handleMove = (me: MouseEvent) => {
      const dx = me.clientX - state.startX;
      const dy = me.clientY - state.startY;
      if (!state.started && dx * dx + dy * dy > DRAG_THRESHOLD_SQ) {
        state.started = true;
        const urls = hashes.slice(0, 3).map(h => mediaThumbnailUrl(h));
        imageDrag.start(hashes, urls, me.clientX, me.clientY);
      }
      if (state.started) {
        imageDrag.move(me.clientX, me.clientY);
      }
    };

    const handleUp = () => {
      window.removeEventListener('mousemove', handleMove);
      window.removeEventListener('mouseup', handleUp);
      if (state.started) {
        imageDrag.end();
      }
    };

    window.addEventListener('mousemove', handleMove);
    window.addEventListener('mouseup', handleUp);
  }, []);

  const handleGridClick = useCallback((e: React.MouseEvent) => {
    if (dragStateRef.current?.started) {
      dragStateRef.current = null;
      return;
    }
    dragStateRef.current = null;
    if ((e.target as HTMLElement).closest(`.${styles.zoomBtn}`)) return;
    const tileEl = (e.target as HTMLElement).closest(`.${styles.tile}`) as HTMLElement | null;
    if (!tileEl) return;
    const wrapperEl = tileEl.closest('[data-hash]') as HTMLElement | null;
    if (!wrapperEl) return;
    const hash = wrapperEl.dataset.hash!;
    const image = imagesByHashRef.current.get(hash);
    if (image) onImageClickRef.current(image, e);
  }, []);

  const handleGridMouseOver = useCallback((e: React.MouseEvent) => {
    if (!(e.target as HTMLElement).classList.contains(styles.zoomBtn)) return;
    const wrapperEl = (e.target as HTMLElement).closest('[data-hash]') as HTMLElement | null;
    if (!wrapperEl) return;
    const mime = wrapperEl.dataset.mime ?? '';
    const hash = wrapperEl.dataset.hash!;
    if (isVideoMime(mime)) return;
    const image = imagesByHashRef.current.get(hash);
    if (image?.is_collection) return;
    handleHoverPreview(hash, mime);
  }, [handleHoverPreview]);

  const handleGridMouseOut = useCallback((e: React.MouseEvent) => {
    if (!(e.target as HTMLElement).classList.contains(styles.zoomBtn)) return;
    handleHoverPreviewHide();
  }, [handleHoverPreviewHide]);

  const handleGridMouseMove = useCallback((e: React.MouseEvent) => {
    const wrapperEl = (e.target as HTMLElement).closest('[data-hash]') as HTMLElement | null;
    const hash = wrapperEl?.dataset.hash ?? null;

    if (hash !== videoScrubHashRef.current) {
      if (videoScrubTimerRef.current) {
        clearTimeout(videoScrubTimerRef.current);
        videoScrubTimerRef.current = null;
      }
      videoScrubHashRef.current = hash;
      setVideoScrub(null);

      if (hash && wrapperEl) {
        const mime = wrapperEl.dataset.mime ?? '';
        const image = imagesByHashRef.current.get(hash);
        if (image && isVideoMime(mime) && image.duration_ms && image.duration_ms > 0) {
          videoScrubTimerRef.current = setTimeout(() => {
            videoScrubTimerRef.current = null;
            const tileEl = wrapperEl.querySelector(`.${styles.tile}`) as HTMLElement | null;
            if (!tileEl) return;
            const tileRect = tileEl.getBoundingClientRect();
            setVideoScrub({
              hash: image.hash,
              mime: image.mime,
              durationSec: image.duration_ms! / 1000,
              rect: {
                left: tileRect.left,
                top: tileRect.top,
                width: tileRect.width,
                height: tileRect.height,
              },
            });
          }, VIDEO_SCRUB_DELAY_MS);
        }
      }
    }
  }, []);

  const handleGridMouseLeave = useCallback(() => {
    // Don't dismiss active overlay here -- mouse-leave fires when entering
    // the overlay portal. The overlay's own onMouseLeave handles dismiss.
    if (videoScrubTimerRef.current) {
      clearTimeout(videoScrubTimerRef.current);
      videoScrubTimerRef.current = null;
    }
  }, []);

  useEffect(() => {
    onContainerWidthChange?.(containerWidth);
  }, [containerWidth, onContainerWidthChange]);

  const textHeight = computeTextHeight(showTileName, showResolution);

  const layout = useMemo(
    () => computeLayout(images, containerWidth, targetSize, gap, viewMode, textHeight),
    [images, containerWidth, targetSize, gap, viewMode, textHeight],
  );

  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const viewModeRef = useRef(viewMode);
  viewModeRef.current = viewMode;
  const marqueeActiveRef = useRef(marqueeActive);
  marqueeActiveRef.current = marqueeActive;

  const imagesRef = useRef(images);
  imagesRef.current = images;

  // Trigger loads when tiles mount; purge stale queue entries first
  useEffect(() => {
    const container = perfContainerRef.current;
    if (!container) return;

    purgeLoadQueue();
    const wrappers = container.querySelectorAll('[data-hash]');
    for (let i = 0; i < wrappers.length; i++) {
      const img = wrappers[i].querySelector('img[data-src]') as HTMLImageElement | null;
      if (!img) continue;
      const dataSrc = img.dataset.src!;
      if (thumbReadyCache.has(dataSrc) || img.src) continue;

      scheduleLoad(img, dataSrc);

      // Already in browser cache → show immediately
      if (img.complete && img.naturalHeight > 0) {
        thumbReadyCache.add(dataSrc);
        onLoadSlotFree();
        img.style.transition = 'none';
        img.style.opacity = '1';
        const prev = img.previousElementSibling as HTMLElement | null;
        if (prev) {
          prev.style.transition = 'none';
          prev.style.opacity = '0';
        }
      }
    }
  }, [visibleIndices]);

  const recomputeVisible = useCallback(() => {
    const positions = layoutRef.current.positions;
    const vh = viewportHeightRef.current;

    if (positions.length === 0 || vh === 0) {
      if (prevRangeKeyRef.current !== '') {
        prevRangeKeyRef.current = '';
        setVisibleIndices([]);
      }
      return;
    }

    const st = scrollTopRef.current;
    const top = st - OVERSCAN_PX;
    const bottom = st + vh + OVERSCAN_PX;

    let indices: number[];

    if (viewModeRef.current !== 'waterfall') {
      // Grid and justified: row-ordered (non-decreasing y) → binary search
      const start = lowerBound(positions, top, (p) => p.y + p.h);
      const endExclusive = lowerBound(positions, bottom, (p) => p.y);
      if (start >= endExclusive) {
        indices = [];
      } else {
        const count = endExclusive - start;
        indices = new Array<number>(count);
        for (let i = 0; i < count; i++) indices[i] = start + i;
      }
    } else {
      // Waterfall: not globally y-sorted → full scan
      indices = [];
      for (let i = 0; i < positions.length; i++) {
        const pos = positions[i];
        if (pos.y + pos.h > top && pos.y < bottom) {
          indices.push(i);
        }
      }
    }

    const key = indices.length === 0
      ? ''
      : `${indices[0]}-${indices[indices.length - 1]}-${indices.length}`;
    if (key !== prevRangeKeyRef.current) {
      prevRangeKeyRef.current = key;
      setVisibleIndices(indices);
    }
  }, []);

  useEffect(() => {
    recomputeVisible();
  }, [layout, recomputeVisible]);

  // Pre-warm blurhash cache in idle chunks so it never blocks a frame
  useEffect(() => {
    const positions = layout.positions;
    const imgs = images;
    if (positions.length === 0) return;

    const BATCH_TARGET = Math.min(300, positions.length);
    let cursor = 0;
    let idleId: number;

    const warmChunk = (deadline: IdleDeadline) => {
      while (cursor < BATCH_TARGET && deadline.timeRemaining() > 1) {
        const img = imgs[cursor];
        if (img?.blurhash) getCachedBlurhash(img.blurhash, img.aspectRatio);
        cursor++;
      }
      if (cursor < BATCH_TARGET) {
        idleId = requestIdleCallback(warmChunk);
      }
    };

    if (typeof requestIdleCallback !== 'undefined') {
      idleId = requestIdleCallback(warmChunk);
    }
    return () => { if (idleId) cancelIdleCallback(idleId); };
  }, [layout, images]);

  useEffect(() => {
    const el = perfContainerRef.current;
    if (!el) return;
    if (marqueeActive) {
      el.setAttribute('data-perf-mode', '1');
    } else if (!isScrollingRef.current) {
      el.removeAttribute('data-perf-mode');
    }
  }, [marqueeActive]);

  const lastComputedScrollRef = useRef(0);

  // rAF scroll listener -- zero React reconciliation during normal scroll
  useEffect(() => {
    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) return;

    viewportHeightRef.current = scrollEl.clientHeight;
    scrollTopRef.current = scrollEl.scrollTop;
    lastComputedScrollRef.current = scrollEl.scrollTop;
    recomputeVisible();

    let rafId = 0;
    let scrollIdleTimer = 0;

    const onScroll = () => {
      if (!isScrollingRef.current) {
        isScrollingRef.current = true;
        perfContainerRef.current?.setAttribute('data-perf-mode', '1');
        if (hoverTimerRef.current) {
          clearTimeout(hoverTimerRef.current);
          hoverTimerRef.current = null;
        }
        if (videoScrubTimerRef.current) {
          clearTimeout(videoScrubTimerRef.current);
          videoScrubTimerRef.current = null;
        }
        videoScrubHashRef.current = null;
        setVideoScrub((prev) => (prev ? null : prev));
      }
      if (scrollIdleTimer) window.clearTimeout(scrollIdleTimer);
      scrollIdleTimer = window.setTimeout(() => {
        isScrollingRef.current = false;
        if (!marqueeActiveRef.current) {
          perfContainerRef.current?.removeAttribute('data-perf-mode');
        }
        // Catch-up: final recomputeVisible for tile mount/unmount
        recomputeVisible();
        lastComputedScrollRef.current = scrollTopRef.current;

        // Pre-warm blurhash cache for nearby off-screen tiles (deadline-batched)
        if (typeof requestIdleCallback !== 'undefined') {
          const st = scrollTopRef.current;
          const vh = viewportHeightRef.current;
          const warmTop = st - 5000;
          const warmBottom = st + vh + 5000;
          const positions = layoutRef.current.positions;
          const imgs = imagesRef.current;
          let cursor = 0;
          const warmChunk = (deadline: IdleDeadline) => {
            while (cursor < positions.length && deadline.timeRemaining() > 1) {
              const pos = positions[cursor];
              if (pos.y + pos.h > warmTop && pos.y < warmBottom) {
                const img = imgs[cursor];
                if (img?.blurhash) getCachedBlurhash(img.blurhash, img.aspectRatio);
              }
              cursor++;
            }
            if (cursor < positions.length) requestIdleCallback(warmChunk);
          };
          requestIdleCallback(warmChunk);
        }
      }, 150);

      if (rafId) return;
      rafId = requestAnimationFrame(() => {
        rafId = 0;
        scrollTopRef.current = scrollEl.scrollTop;
        // Recompute when we've consumed 40% of the overscan buffer.
        // This keeps ~60% (1800px) of buffer ahead at all times.
        const delta = Math.abs(scrollTopRef.current - lastComputedScrollRef.current);
        if (delta > OVERSCAN_PX * 0.25) {
          lastComputedScrollRef.current = scrollTopRef.current;
          recomputeVisible();
        }
      });
    };

    const onResize = () => {
      viewportHeightRef.current = scrollEl.clientHeight;
      recomputeVisible();
    };

    scrollEl.addEventListener('scroll', onScroll, { passive: true });
    const ro = new ResizeObserver(onResize);
    ro.observe(scrollEl);

    return () => {
      scrollEl.removeEventListener('scroll', onScroll);
      ro.disconnect();
      if (rafId) cancelAnimationFrame(rafId);
      if (scrollIdleTimer) window.clearTimeout(scrollIdleTimer);
    };
  }, [scrollContainerRef, recomputeVisible]);

  // Pop animation: when returning from detail view, briefly scale up the tile then shrink back
  useEffect(() => {
    if (!popHash) return;
    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) { onPopComplete?.(); return; }

    // Use rAF to ensure the grid has rendered with the tile visible
    const raf = requestAnimationFrame(() => {
      const tile = scrollEl.querySelector<HTMLElement>(`[data-hash="${popHash}"]`);
      if (!tile) { onPopComplete?.(); return; }

      // Scroll tile into view if needed
      tile.scrollIntoView({ block: 'nearest' });

      const inner = tile.firstElementChild as HTMLElement | null;
      if (!inner) { onPopComplete?.(); return; }

      // Start scaled up, shrink back
      inner.style.transition = 'none';
      inner.style.transform = 'scale(1.08)';
      // Force reflow
      inner.getBoundingClientRect();
      inner.style.transition = 'transform 60ms ease-out';
      inner.style.transform = 'scale(1)';

      const cleanup = () => {
        inner.style.transition = '';
        inner.style.transform = '';
        onPopComplete?.();
      };
      inner.addEventListener('transitionend', cleanup, { once: true });
      // Fallback in case transitionend doesn't fire
      setTimeout(cleanup, 80);
    });

    return () => cancelAnimationFrame(raf);
  }, [popHash, scrollContainerRef, onPopComplete]);

  // Infinite scroll sentinel via IntersectionObserver
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const onLoadMoreRef = useRef(onLoadMore);
  onLoadMoreRef.current = onLoadMore;

  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!sentinel) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          onLoadMoreRef.current?.();
        }
      },
      { rootMargin: '400px' },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [images.length]);

  // Not yet measured — nothing to render
  if (containerWidth === 0) {
    return <div ref={containerRef} style={{ minHeight: 1 }} />;
  }

  // Empty state
  if (images.length === 0) {
    if (!showEmptyState) {
      return <div ref={containerRef} style={{ minHeight: 1 }} />;
    }
    return (
      <div ref={containerRef}>
        <div style={{ padding: '80px 0' }}>
          <EmptyState
            icon={IconPhoto}
            title="No images found"
            description={searchTags?.length
              ? 'Try different search terms'
              : 'Import images to get started'}
            action={
              <TextButton onClick={onImport}>
                <IconUpload size={14} />
                Import Images
              </TextButton>
            }
          />
        </div>
      </div>
    );
  }

  return (
    <div ref={containerRef}>
      <div
        ref={perfContainerRef}
        onMouseDown={handleGridMouseDown}
        onClick={handleGridClick}
        onMouseOver={handleGridMouseOver}
        onMouseOut={handleGridMouseOut}
        onMouseMove={handleGridMouseMove}
        onMouseLeave={handleGridMouseLeave}
        style={{
          position: 'relative',
          height: layout.totalHeight,
          width: '100%',
        }}
      >
        {visibleIndices.map((i) => {
          const image = images[i];
          if (!image) return null;
          return (
            <VirtualTile
              key={image.hash}
              image={image}
              layout={layout.positions[i]}
              isSelected={selectedHashes.has(image.hash)}
              textHeight={textHeight}
              showTileName={showTileName}
              showResolution={showResolution}
              showExtension={showExtension}
              showExtensionLabel={showExtensionLabel}
              thumbnailFitMode={thumbnailFitMode}
            />
          );
        })}
      </div>
      {onLoadMore && <div ref={sentinelRef} style={{ height: 1 }} />}
      {hoverPreview && <HoverPreview {...hoverPreview} />}
      {videoScrub && (
        <VideoScrubOverlay
          tileRect={videoScrub.rect}
          src={mediaFileUrl(videoScrub.hash, videoScrub.mime)}
          duration={videoScrub.durationSec}
          onDismiss={() => {
            videoScrubHashRef.current = null;
            setVideoScrub(null);
          }}
        />
      )}
    </div>
  );
}
