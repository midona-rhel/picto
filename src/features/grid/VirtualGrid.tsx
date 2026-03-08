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
import { IconFolderPlus, IconPhoto, IconUpload } from '@tabler/icons-react';
import { EmptyState } from '../../shared/components/EmptyState';
import { TextButton } from '../../shared/components/TextButton';
import { decode } from 'blurhash';
import { MasonryImageItem, isVideoMime } from './shared';
import { VideoScrubOverlay, type VideoScrubRect } from './VideoScrubOverlay';
import { mediaThumbnailUrl, mediaFileUrl } from '../../shared/lib/mediaUrl';
import { formatDuration } from '../../shared/lib/formatters';
import { imageDrag } from '../../shared/lib/imageDrag';
import type { GridEmptyContext, GridViewMode } from './runtime';
import {
  hasActivatedThumb,
  isThumbReady,
  useGridLazyLoadManager,
  wasThumbLoadedFromCache,
} from './gridLazyLoadManager';
import {
  computeLayout,
  computeTextHeight,
  lowerBound,
  type LayoutItem,
} from './gridLayout';
import styles from './VirtualGrid.module.css';

const THUMB_MAX_SIDE = 900;
const DRAG_THRESHOLD_SQ = 25; // 5px²
const OVERSCAN_PX = 5000;
const LOAD_MORE_THRESHOLD_PX = 2000;

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

interface TileProps {
  lazyLoadManager: ReturnType<typeof useGridLazyLoadManager>;
  image: MasonryImageItem;
  layout: LayoutItem;
  isSelected: boolean;
  isBoxSelected: boolean;
  textHeight: number;
  showTileName?: boolean;
  showResolution?: boolean;
  showExtension?: boolean;
  showExtensionLabel?: boolean;
  thumbnailFitMode?: 'cover' | 'contain';
  isRenaming?: boolean;
  renameValue?: string;
  renameInputRef?: RefObject<HTMLInputElement>;
  onRenameChange?: (value: string) => void;
  onRenameCommit?: () => void;
  onRenameCancel?: () => void;
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
  lazyLoadManager,
  image,
  layout,
  isSelected,
  isBoxSelected,
  textHeight,
  showTileName = true,
  showResolution = true,
  showExtension = true,
  showExtensionLabel = true,
  thumbnailFitMode = 'cover',
  isRenaming = false,
  renameValue = '',
  renameInputRef,
  onRenameChange,
  onRenameCommit,
  onRenameCancel,
}: TileProps) {
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const blurhashUrl = getCachedBlurhash(image.blurhash, image.aspectRatio);
  const imgUrl = layout.w > THUMB_MAX_SIDE
    ? mediaFileUrl(image.hash, image.mime)
    : mediaThumbnailUrl(image.hash);

  const ext = mimeToExt(image.mime);
  const isVideo = image.mime.startsWith('video/');
  const isAnimated = image.mime === 'image/gif' && (image.num_frames ?? 0) > 1;
  const isCollection = image.is_collection === true;
  const durationMs = image.duration_ms;
  const showBadge = !isCollection && showExtensionLabel && ext && !BADGE_HIDDEN_TYPES.has(ext.toLowerCase());

  const imageHeight = layout.h - textHeight;

  const fullyLoaded = isThumbReady(imgUrl);
  const hasSrc = fullyLoaded || hasActivatedThumb(imgUrl);
  const loadedFromCache = fullyLoaded && wasThumbLoadedFromCache(imgUrl);

  useEffect(() => {
    const wrapper = wrapperRef.current;
    if (!wrapper) return;
    lazyLoadManager.observeTile(wrapper);
    return () => lazyLoadManager.unobserveTile(wrapper);
  }, [imgUrl, lazyLoadManager]);

  return (
    <div
      ref={wrapperRef}
      data-hash={image.hash}
      data-mime={image.mime}
      data-pos-y={Math.round(layout.y)}
      data-box-selected={isBoxSelected ? '1' : undefined}
      data-thumb-state={fullyLoaded ? 'loaded' : 'idle'}
      data-thumb-cache={loadedFromCache ? '1' : undefined}
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
          onLoad={lazyLoadManager.handleImageLoad}
          onError={lazyLoadManager.handleImageError}
          className={styles.thumbLayer}
          style={fullyLoaded
            ? thumbnailFitMode === 'contain' ? { opacity: 1, objectFit: 'contain' } : { opacity: 1 }
            : thumbnailFitMode === 'contain' ? { objectFit: 'contain' } : undefined}
        />
        {showBadge && <span className={styles.extensionBadge}>{ext}</span>}
        {(isVideo || isAnimated) && typeof durationMs === 'number' && durationMs > 0 && (
          <span className={styles.durationBadge}>{formatDuration(durationMs)}</span>
        )}
        {isCollection && <span className={styles.extensionBadge}>collection</span>}
        {isCollection && (
          <span className={styles.durationBadge}>
            {`${Math.max(0, image.collection_item_count ?? 0)} items`}
          </span>
        )}
        {!isCollection && <span className={styles.zoomBtn} />}
      </div>
      {showTileName && isRenaming ? (
        <input
          ref={renameInputRef}
          value={renameValue}
          onChange={(e) => onRenameChange?.(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') onRenameCommit?.();
            if (e.key === 'Escape') onRenameCancel?.();
          }}
          onBlur={() => onRenameCommit?.()}
          className={styles.tileName}
          style={{
            width: '100%',
            border: '1px solid var(--color-primary)',
            borderRadius: 3,
            background: 'var(--color-bg-primary, #1e1e1e)',
            color: 'var(--color-text-primary)',
            outline: 'none',
          }}
        />
      ) : showTileName ? (
        <div className={styles.tileName} title={image.name || ''}>
          {image.name || 'Untitled'}{showExtension && ext ? `.${ext}` : ''}
        </div>
      ) : null}
      {!isRenaming && showTileName && showResolution && image.width && image.height && (
        <div className={styles.tileInfo}>
          {image.width} × {image.height}
        </div>
      )}
    </div>
  );
}, (prev, next) =>
  prev.image === next.image && prev.layout === next.layout &&
  prev.isSelected === next.isSelected && prev.isBoxSelected === next.isBoxSelected &&
  prev.textHeight === next.textHeight &&
  prev.showTileName === next.showTileName && prev.showResolution === next.showResolution &&
  prev.showExtension === next.showExtension && prev.showExtensionLabel === next.showExtensionLabel &&
  prev.thumbnailFitMode === next.thumbnailFitMode &&
  prev.isRenaming === next.isRenaming &&
  prev.renameValue === next.renameValue
);

interface HoverPreviewData {
  hash: string;
  mime: string;
}

const PREVIEW_DELAY_MS = 200;
const VIDEO_SCRUB_DELAY_MS = 500;
const PREVIEW_INSET = 48;

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
  onImportFolder?: (() => void) | undefined;
  onContainerWidthChange?: (width: number) => void;
  showEmptyState?: boolean;
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
  thumbnailFitMode?: 'cover' | 'contain';
  marqueeRectRef?: React.MutableRefObject<{ left: number; top: number; width: number; height: number } | null>;
  marqueeHitHashesRef?: React.MutableRefObject<Set<string> | null>;
  scheduleRedrawRef?: React.MutableRefObject<(() => void) | null>;
  onLayoutChange?: (positions: LayoutItem[]) => void;
  reorderMode?: boolean;
  onReorder?: (movedHashes: string[], targetIndex: number) => void;
  totalCount?: number | null;
  dragDisabled?: boolean;
  renamingHash?: string | null;
  renameValue?: string;
  renameInputRef?: RefObject<HTMLInputElement>;
  onRenameChange?: (value: string) => void;
  onRenameCommit?: () => void;
  onRenameCancel?: () => void;
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
  onImportFolder,
  onContainerWidthChange,
  showEmptyState = true,
  emptyContext = 'default',
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
  marqueeRectRef,
  marqueeHitHashesRef,
  scheduleRedrawRef,
  onLayoutChange,
  reorderMode = false,
  onReorder,
  dragDisabled = false,
  renamingHash = null,
  renameValue = '',
  renameInputRef,
  onRenameChange,
  onRenameCommit,
  onRenameCancel,
}: VirtualGridProps) {
  const { ref: containerRef, width: containerWidth } = useContainerWidth();
  const lazyLoadManager = useGridLazyLoadManager(scrollContainerRef);

  // Scroll state in refs to avoid re-renders per scroll frame
  const scrollTopRef = useRef(0);
  const viewportHeightRef = useRef(0);
  const isScrollingRef = useRef(false);
  const perfContainerRef = useRef<HTMLDivElement>(null);
  const prevRangeKeyRef = useRef('');
  const renderTickRef = useRef(0);
  const [, setRenderTick] = useState(0);

  const dragStateRef = useRef<{ hash: string; startX: number; startY: number; started: boolean } | null>(null);
  const reorderStateRef = useRef<{
    sourceHash: string;
    hashes: string[];
    startX: number;
    startY: number;
    started: boolean;
    dropIndex: number | null;
  } | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);

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
  const onReorderRef = useRef(onReorder);
  onReorderRef.current = onReorder;

  const forceOverlayRender = useCallback(() => {
    renderTickRef.current += 1;
    setRenderTick(renderTickRef.current);
  }, []);

  useEffect(() => {
    if (!scheduleRedrawRef) return;
    scheduleRedrawRef.current = forceOverlayRender;
    return () => {
      if (scheduleRedrawRef.current === forceOverlayRender) {
        scheduleRedrawRef.current = null;
      }
    };
  }, [forceOverlayRender, scheduleRedrawRef]);

  const computeDropIndex = useCallback((clientX: number, clientY: number): number | null => {
    const scrollEl = scrollContainerRef?.current;
    const surface = perfContainerRef.current;
    if (!scrollEl || !surface) return null;

    const scrollRect = scrollEl.getBoundingClientRect();
    const surfaceRect = surface.getBoundingClientRect();
    const localX = clientX - scrollRect.left + scrollEl.scrollLeft;
    const localY = clientY - scrollRect.top + scrollEl.scrollTop - (surfaceRect.top - scrollRect.top);
    const positions = layoutRef.current.positions;
    const draggedHashes = new Set(reorderStateRef.current?.hashes ?? []);

    for (let i = 0; i < positions.length; i++) {
      const pos = positions[i];
      if (localX >= pos.x && localX < pos.x + pos.w && localY >= pos.y && localY < pos.y + pos.h) {
        const image = imagesRef.current[i];
        if (image && draggedHashes.has(image.hash)) return null;
        const before = localX < pos.x + pos.w / 2;
        return before ? i : i + 1;
      }
    }

    if (positions.length === 0) return 0;
    const last = positions[positions.length - 1];
    if (localY >= last.y) {
      return localX < last.x + last.w / 2 ? positions.length - 1 : positions.length;
    }
    return null;
  }, [scrollContainerRef]);

  // --- Delegated handlers
  const handleGridPointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const tileEl = (e.target as HTMLElement).closest(`.${styles.tile}`) as HTMLElement | null;
    if (!tileEl) return;
    const wrapperEl = tileEl.closest('[data-hash]') as HTMLElement | null;
    if (!wrapperEl) return;
    const hash = wrapperEl.dataset.hash!;
    const image = imagesByHashRef.current.get(hash);
    if (!image) return;

    e.stopPropagation();
    if ((e.target as HTMLElement).closest(`.${styles.zoomBtn}`)) return;
    if (dragDisabled) return;

    const selectedList = selectedHashesRef.current.has(hash)
      ? Array.from(selectedHashesRef.current)
      : [hash];

    if (reorderMode && onReorderRef.current) {
      reorderStateRef.current = {
        sourceHash: hash,
        hashes: selectedList,
        startX: e.clientX,
        startY: e.clientY,
        started: false,
        dropIndex: null,
      };

      const handleMove = (me: PointerEvent) => {
        const state = reorderStateRef.current;
        if (!state) return;
        const dx = me.clientX - state.startX;
        const dy = me.clientY - state.startY;
        if (!state.started && dx * dx + dy * dy > DRAG_THRESHOLD_SQ) {
          state.started = true;
        }
        if (!state.started) return;
        const nextDropIndex = computeDropIndex(me.clientX, me.clientY);
        if (nextDropIndex !== state.dropIndex) {
          state.dropIndex = nextDropIndex;
          setDropIndex(nextDropIndex);
        }
      };

      const handleUp = () => {
        window.removeEventListener('pointermove', handleMove);
        window.removeEventListener('pointerup', handleUp);
        const state = reorderStateRef.current;
        reorderStateRef.current = null;
        setDropIndex(null);
        if (!state?.started || state.dropIndex == null) return;
        onReorderRef.current?.(state.hashes, state.dropIndex);
      };

      window.addEventListener('pointermove', handleMove);
      window.addEventListener('pointerup', handleUp);
      return;
    }

    const state = { hash, startX: e.clientX, startY: e.clientY, started: false };
    dragStateRef.current = state;

    const sel = imageDrag.getSelectedHashes();
    const isSelected = selectedHashesRef.current.has(hash);
    const hashes = isSelected && sel.size > 0 ? Array.from(sel) : [hash];

    const handleMove = (me: PointerEvent) => {
      const dx = me.clientX - state.startX;
      const dy = me.clientY - state.startY;
      if (!state.started && dx * dx + dy * dy > DRAG_THRESHOLD_SQ) {
        state.started = true;
        const urls = hashes.slice(0, 3).map((h) => mediaThumbnailUrl(h));
        imageDrag.start(hashes, urls, me.clientX, me.clientY);
      }
      if (state.started) {
        imageDrag.move(me.clientX, me.clientY);
      }
    };

    const handleUp = () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
      if (state.started) {
        imageDrag.end();
      }
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  }, [computeDropIndex, dragDisabled, reorderMode]);

  const handleGridClick = useCallback((e: React.MouseEvent) => {
    if (dragStateRef.current?.started) {
      dragStateRef.current = null;
      return;
    }
    if (reorderStateRef.current?.started) {
      reorderStateRef.current = null;
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

  useEffect(() => {
    onLayoutChange?.(layout.positions);
  }, [layout.positions, onLayoutChange]);

  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const viewModeRef = useRef(viewMode);
  viewModeRef.current = viewMode;
  const marqueeActiveRef = useRef(marqueeActive);
  marqueeActiveRef.current = marqueeActive;

  const imagesRef = useRef(images);
  imagesRef.current = images;

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

    const maybeLoadMore = () => {
      if (!onLoadMoreRef.current) return;
      const distanceToEnd = layoutRef.current.totalHeight - (scrollTopRef.current + viewportHeightRef.current);
      if (distanceToEnd <= LOAD_MORE_THRESHOLD_PX) {
        onLoadMoreRef.current();
      }
    };
    maybeLoadMore();

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
        maybeLoadMore();
      });
    };

    const onResize = () => {
      viewportHeightRef.current = scrollEl.clientHeight;
      recomputeVisible();
      maybeLoadMore();
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

  const onLoadMoreRef = useRef(onLoadMore);
  onLoadMoreRef.current = onLoadMore;

  // Not yet measured — nothing to render
  if (containerWidth === 0) {
    return <div ref={containerRef} style={{ minHeight: 1 }} />;
  }

  // Empty state
  if (images.length === 0) {
    if (!showEmptyState) {
      return <div ref={containerRef} style={{ minHeight: 1 }} />;
    }
    const hasSearchTerms = (searchTags?.length ?? 0) > 0;
    return (
      <div ref={containerRef}>
        <div style={{ padding: '80px 0' }}>
          <EmptyState
            icon={IconPhoto}
            title={getEmptyStateTitle(emptyContext, hasSearchTerms)}
            description={getEmptyStateDescription(emptyContext, hasSearchTerms)}
            action={
              onImportFolder && emptyContext === 'folder' ? (
                <div style={{ display: 'flex', gap: 8 }}>
                  <TextButton onClick={onImport}>
                    <IconUpload size={14} />
                    Import Images
                  </TextButton>
                  <TextButton onClick={onImportFolder}>
                    <IconFolderPlus size={14} />
                    Import Folder
                  </TextButton>
                </div>
              ) : (
                <TextButton onClick={onImport}>
                  <IconUpload size={14} />
                  Import Images
                </TextButton>
              )
            }
          />
        </div>
      </div>
    );
  }

  const liveMarqueeRect = marqueeRectRef?.current ?? null;
  const liveMarqueeHits = marqueeHitHashesRef?.current ?? null;
  let dropIndicatorStyle: React.CSSProperties | null = null;
  if (dropIndex != null) {
    if (dropIndex >= layout.positions.length && layout.positions.length > 0) {
      const last = layout.positions[layout.positions.length - 1];
      dropIndicatorStyle = {
        position: 'absolute',
        left: last.x + last.w,
        top: last.y,
        width: 3,
        height: last.h,
        background: 'var(--color-primary)',
        pointerEvents: 'none',
        borderRadius: 999,
        transform: 'translateX(-1px)',
      };
    } else {
      const pos = layout.positions[Math.max(0, dropIndex)];
      if (pos) {
        dropIndicatorStyle = {
          position: 'absolute',
          left: pos.x,
          top: pos.y,
          width: 3,
          height: pos.h,
          background: 'var(--color-primary)',
          pointerEvents: 'none',
          borderRadius: 999,
          transform: 'translateX(-1px)',
        };
      }
    }
  }

  return (
    <div ref={containerRef}>
      <div
        ref={perfContainerRef}
        data-grid-surface-root
        onPointerDown={handleGridPointerDown}
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
              lazyLoadManager={lazyLoadManager}
              image={image}
              layout={layout.positions[i]}
              isSelected={selectedHashes.has(image.hash)}
              isBoxSelected={liveMarqueeHits?.has(image.hash) ?? false}
              textHeight={textHeight}
              showTileName={showTileName}
              showResolution={showResolution}
              showExtension={showExtension}
              showExtensionLabel={showExtensionLabel}
              thumbnailFitMode={thumbnailFitMode}
              isRenaming={renamingHash === image.hash}
              renameValue={renameValue}
              renameInputRef={renameInputRef}
              onRenameChange={onRenameChange}
              onRenameCommit={onRenameCommit}
              onRenameCancel={onRenameCancel}
            />
          );
        })}
        {liveMarqueeRect && (
          <div
            style={{
              position: 'absolute',
              left: liveMarqueeRect.left,
              top: liveMarqueeRect.top,
              width: liveMarqueeRect.width,
              height: liveMarqueeRect.height,
              background: 'rgba(80, 140, 255, 0.14)',
              border: '1px solid rgba(80, 140, 255, 0.72)',
              pointerEvents: 'none',
            }}
          />
        )}
        {dropIndicatorStyle && <div style={dropIndicatorStyle} />}
      </div>
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
