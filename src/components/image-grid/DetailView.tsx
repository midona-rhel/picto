import { useState, useEffect, useLayoutEffect, useRef, useCallback, useMemo } from 'react';
import { api } from '#desktop/api';
import { IconCheck, IconX } from '@tabler/icons-react';
import { KbdTooltip } from '../../shared/components/KbdTooltip';
import { MasonryImageItem, isVideoMime, toMasonryItem } from './shared';
import { VideoPlayer } from '../video/VideoPlayer';
import { StripView } from './StripView';
import { GridController } from '../../controllers/gridController';
import { useSettingsStore } from '../../state/settingsStore';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { getCachedMediaUrl } from './enhancedMediaCache';
import { useImageZoom, type ImageSize, type ZoomState } from './useImageZoom';
import { useNavigatorDrag } from './useNavigatorDrag';
import { useNavigatorRenderer } from './useNavigatorRenderer';
import { useBoundaryNavigation } from '../../shared/hooks/useBoundaryNavigation';
import { FileController } from '../../controllers/fileController';
import { getShortcut, matchesShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { runCriticalAction } from '../../shared/lib/asyncOps';
import { useViewerMediaPipeline } from './viewer/useViewerMediaPipeline';
import styles from './DetailView.module.css';
import shared from './imageViewer.module.css';

// Shared interface for titlebar to read
export interface DetailViewState {
  currentIndex: number;
  total: number;
  zoomPercent: number;
  zoomScale: number;
  fitScale: number;
  isStripMode: boolean;
}

export interface DetailViewControls {
  close: () => void;
  navigate: (direction: number) => void;
  setZoomScale: (scale: number) => void;
  fitToWindow: () => void;
  fitActual: () => void;
}

interface DetailViewProps {
  images: MasonryImageItem[];
  currentIndex: number;
  onNavigate: (delta: number) => void;
  onClose: (exitHash: string) => void;
  onStateChange?: (state: DetailViewState, controls: DetailViewControls) => void;
  onImageChange?: (hash: string) => void;
  onLoadMore?: () => void;
  /** Actual total count of images in the scope (may exceed images.length if not all pages are loaded) */
  totalCount?: number | null;
  /** When true, shows accept/reject buttons and remaps arrow keys */
  inboxMode?: boolean;
  /** Called when user accepts or rejects an image in inbox mode */
  onInboxAction?: (hash: string, status: 'active' | 'trash') => void;
}

// Per-image zoom state cache
const zoomCache = new Map<string, ZoomState>();

const NAV_SIZE = 120;

const COLLECTION_PAGE_SIZE = 100;

export function DetailView({ images, currentIndex, onNavigate, onClose, onStateChange, onImageChange, onLoadMore, totalCount, inboxMode, onInboxAction }: DetailViewProps) {
  // Close when images list empties (all inbox images reviewed)
  const prevImagesLenRef = useRef(images.length);
  useEffect(() => {
    const prevLen = prevImagesLenRef.current;
    prevImagesLenRef.current = images.length;
    if (images.length === 0 && prevLen > 0) {
      onClose('');
    }
  }, [images.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const currentImage = images[currentIndex];
  const isCollection = !!(currentImage?.is_collection && currentImage?.entity_id != null);
  const isVideo = currentImage ? isVideoMime(currentImage.mime) : false;

  const stripMode = isCollection;
  const [collectionImages, setCollectionImages] = useState<MasonryImageItem[]>([]);
  const collectionCursorRef = useRef<string | null>(null);
  const collectionHasMoreRef = useRef(true);
  const collectionLoadingRef = useRef(false);
  const lastCollectionIdRef = useRef<number | null>(null);

  // Fetch collection members
  const loadCollectionPage = useCallback(
    async (entityId: number, cursor: string | null) => {
      if (collectionLoadingRef.current) return;
      collectionLoadingRef.current = true;
      try {
        // Detail collection-strip pagination is owned by DetailView, not GridQueryBroker.
        const resp = await GridController.fetchGridPage({
          limit: COLLECTION_PAGE_SIZE,
          cursor,
          sortField: 'ordinal',
          sortOrder: 'asc',
          collectionEntityId: entityId,
        });
        const items = resp.items.map(toMasonryItem);
        setCollectionImages((prev) => (cursor ? [...prev, ...items] : items));
        collectionCursorRef.current = resp.next_cursor;
        collectionHasMoreRef.current = resp.has_more;
      } catch (err) {
        console.error('Failed to load collection members:', err);
      } finally {
        collectionLoadingRef.current = false;
      }
    },
    [],
  );

  // Load collection members when current image is a collection
  useEffect(() => {
    if (!isCollection) {
      lastCollectionIdRef.current = null;
      return;
    }
    const entityId = currentImage!.entity_id!;
    if (entityId === lastCollectionIdRef.current) return;
    lastCollectionIdRef.current = entityId;
    setCollectionImages([]);
    collectionCursorRef.current = null;
    collectionHasMoreRef.current = true;
    loadCollectionPage(entityId, null);
  }, [isCollection, currentImage?.entity_id, loadCollectionPage]);

  const handleStripLoadMore = useCallback(() => {
    if (!isCollection || !collectionHasMoreRef.current) return;
    const entityId = currentImage?.entity_id;
    if (entityId == null) return;
    loadCollectionPage(entityId, collectionCursorRef.current);
  }, [isCollection, currentImage?.entity_id, loadCollectionPage]);

  // Hide backdrop once StripView has content to display.
  // Also capture the cover thumbnail so navigating AWAY from a collection
  // has a valid backdrop image (strip mode bypasses the main img onLoad).
  useEffect(() => {
    if (stripMode && collectionImages.length > 0) {
      setThumbLoaded(true);
      lastLoadedSrcRef.current = mediaThumbnailUrl(collectionImages[0].hash);
    }
  }, [stripMode, collectionImages.length]); // eslint-disable-line react-hooks/exhaustive-deps

  // Keeps the StripView visible until the new image's thumbnail loads,
  // preventing the visual "zoom out" from strip layout to object-fit backdrop.
  const [holdingStrip, setHoldingStrip] = useState(false);
  const prevStripRef = useRef(false);

  const [stripZoomScale, setStripZoomScale] = useState(1);
  const stripZoomRef = useRef(stripZoomScale);
  stripZoomRef.current = stripZoomScale;
  const [stripResetKey, setStripResetKey] = useState(0);

  // Reset strip zoom + scroll when navigating to a new image
  useEffect(() => {
    if (stripMode) {
      setStripZoomScale(1);
      setStripResetKey((k) => k + 1);
    }
  }, [currentIndex]); // eslint-disable-line react-hooks/exhaustive-deps

  const imageSize: ImageSize | null = currentImage?.width && currentImage?.height
    ? { width: currentImage.width, height: currentImage.height }
    : null;

  const containerRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const thumbImgRef = useRef<HTMLImageElement>(null);
  const {
    state: zoomState,
    setState: setZoomState,
    isDragging,
    getFitScale,
    calcFitScale,
    fitToWindow,
    fitActual,
    zoomTo,
    navigatorRect,
    panToNormalized,
    handlers,
  } = useImageZoom(containerRef, imageSize);

  const navigatorRef = useRef<HTMLDivElement>(null);
  const navViewportRef = useRef<HTMLDivElement>(null);

  // Stable refs for controls (avoid stale closures in callbacks)
  const currentIndexRef = useRef(currentIndex);
  currentIndexRef.current = currentIndex;

  // Video ref for keyboard handler
  const isVideoRef = useRef(isVideo);
  isVideoRef.current = isVideo;

  // Current image refs for shortcuts (rating, file ops)
  const currentHashRef = useRef(currentImage?.hash);
  currentHashRef.current = currentImage?.hash;
  const currentImageRef = useRef(currentImage);
  currentImageRef.current = currentImage;

  // Inbox mode refs for keyboard handler
  const inboxModeRef = useRef(inboxMode);
  inboxModeRef.current = inboxMode;
  const onInboxActionRef = useRef(onInboxAction);
  onInboxActionRef.current = onInboxAction;
  const imagesRef = useRef(images);
  imagesRef.current = images;

  const imageUrl = useMemo(() => {
    if (!currentImage) return '';
    return getCachedMediaUrl(currentImage.hash, 'full') || mediaFileUrl(currentImage.hash, currentImage.mime);
  }, [currentImage]);

  // Navigate with boundary flash — adapt delta-based onNavigate to absolute-index API
  const handleNavigateAbsolute = useCallback((newIndex: number) => {
    onNavigate(newIndex - currentIndexRef.current);
  }, [onNavigate]); // eslint-disable-line react-hooks/exhaustive-deps
  const { navigate, boundaryFlash } = useBoundaryNavigation(images.length, handleNavigateAbsolute, currentIndexRef);

  // Close immediately — grid handles the pop animation on the tile
  const handleClose = useCallback(() => {
    const hash = images[currentIndexRef.current]?.hash ?? '';
    onClose(hash);
  }, [images, onClose]);

  // Stable controls object (refs ensure no stale closures and avoid update loops upstream)
  const stripModeRef = useRef(stripMode);
  stripModeRef.current = stripMode;
  const zoomToRef = useRef(zoomTo);
  zoomToRef.current = zoomTo;
  const fitToWindowRef = useRef(fitToWindow);
  fitToWindowRef.current = fitToWindow;
  const fitActualRef = useRef(fitActual);
  fitActualRef.current = fitActual;
  const handleCloseRef = useRef(handleClose);
  handleCloseRef.current = handleClose;
  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;

  const controlsRef = useRef<DetailViewControls | null>(null);
  if (!controlsRef.current) {
    controlsRef.current = {
      close: () => handleCloseRef.current(),
      navigate: (direction: number) => navigateRef.current(direction),
      setZoomScale: (scale: number) => {
        if (stripModeRef.current) {
          setStripZoomScale(Math.max(0.05, Math.min(8, scale)));
          return;
        }
        zoomToRef.current(scale);
      },
      fitToWindow: () => {
        if (stripModeRef.current) {
          setStripZoomScale(1);
          setStripResetKey((k) => k + 1);
          return;
        }
        fitToWindowRef.current();
      },
      fitActual: () => {
        if (stripModeRef.current) {
          setStripZoomScale(1);
          return;
        }
        fitActualRef.current();
      },
    };
  }

  // Report state to parent for titlebar — rAF-throttled to avoid per-frame re-renders
  const activeScale = stripMode ? stripZoomScale : zoomState.scale;
  const stateChangeRafRef = useRef(0);
  const onStateChangeRef = useRef(onStateChange);
  onStateChangeRef.current = onStateChange;
  const getFitScaleRef = useRef(getFitScale);
  getFitScaleRef.current = getFitScale;
  useEffect(() => {
    if (!onStateChangeRef.current) return;
    if (stateChangeRafRef.current) cancelAnimationFrame(stateChangeRafRef.current);
    stateChangeRafRef.current = requestAnimationFrame(() => {
      stateChangeRafRef.current = 0;
      const fitScale = stripMode ? 1 : getFitScaleRef.current();
      onStateChangeRef.current?.(
        {
          currentIndex,
          total: totalCount ?? images.length,
          zoomPercent: Math.round(activeScale * 100),
          zoomScale: activeScale,
          fitScale,
          isStripMode: stripMode,
        },
        controlsRef.current!,
      );
    });
    return () => { if (stateChangeRafRef.current) cancelAnimationFrame(stateChangeRafRef.current); };
  }, [currentIndex, images.length, activeScale, stripMode, totalCount]);

  // Notify parent when the active image changes (for inspector/selection sync).
  // Important: inbox accept/reject can swap the current image while keeping the
  // same index, so this must track hash changes, not only index changes.
  const onImageChangeRef = useRef(onImageChange);
  onImageChangeRef.current = onImageChange;
  const activeImageHash = images[currentIndex]?.hash ?? null;
  useEffect(() => {
    if (activeImageHash) onImageChangeRef.current?.(activeImageHash);
  }, [activeImageHash]);

  // Load more when navigating near the end of loaded images
  const onLoadMoreRef = useRef(onLoadMore);
  onLoadMoreRef.current = onLoadMore;
  useEffect(() => {
    if (currentIndex >= images.length - 5) {
      onLoadMoreRef.current?.();
    }
  }, [currentIndex, images.length]);

  // Update image transform + navigator via direct DOM writes — zero React re-renders during zoom/pan
  const imageSizeRef = useRef(imageSize);
  imageSizeRef.current = imageSize;
  const showMinimap = useSettingsStore(s => s.settings.showMinimap);
  useNavigatorRenderer(imgRef, navigatorRef, navViewportRef, imageSizeRef, zoomState, showMinimap ? navigatorRect : null, NAV_SIZE, thumbImgRef);

  // Keep strip visible during collection -> image transition until thumbnail is ready.
  useLayoutEffect(() => {
    if (prevStripRef.current && !stripMode) {
      setHoldingStrip(true);
    }
    prevStripRef.current = stripMode;
    if (stripMode) {
      setCollectionImages([]);
    }
  }, [currentImage?.hash, stripMode]); // eslint-disable-line react-hooks/exhaustive-deps

  const {
    decodedSrc,
    thumbLoaded,
    fullImageVisible,
    displayThumbUrl,
    allowFullQuality,
    backdropSrcRef,
    lastLoadedSrcRef,
    setThumbLoaded,
    handleThumbLoad,
    handleFullLoad,
  } = useViewerMediaPipeline({
    currentImage: currentImage ?? null,
    images,
    currentIndex,
    imageUrl,
    isVideo,
    imageSize,
    zoomState,
    setZoomState,
    calcFitScale,
    containerRef,
    imgRef,
    zoomCache,
    skipFullQualityDecode: stripMode,
    skipNeighborPrefetch: stripMode,
    ensureThumbLogContext: 'detail.ensureThumbnail',
    preloadThumbLogContext: 'detail.preloadNeighborThumbnail',
  });

  // Release strip hold when new thumbnail loads — strip unmounts, image container revealed.
  useEffect(() => {
    if (holdingStrip && thumbLoaded) {
      setHoldingStrip(false);
      setCollectionImages([]);
    }
  }, [holdingStrip, thumbLoaded]);

  // Handle inbox accept/reject: call action, then auto-advance or close
  const handleInboxAction = useCallback((status: 'active' | 'trash') => {
    const idx = currentIndexRef.current;
    const img = imagesRef.current[idx];
    if (!img || !onInboxActionRef.current) return;
    onInboxActionRef.current(img.hash, status);
    // After removal, images array shrinks. If we were at the last index, need to close or step back.
    // The parent will remove the hash from images, so currentIndex may now point to the next image
    // (or be out of bounds if it was the last). We don't change index here — the parent handles removal.
  }, []);

  // Keyboard — mount once, read everything from refs to avoid stale closures.
  // All bindings go through the central shortcuts registry so they appear in
  // Settings → Shortcuts and can be customised by the user.
  useEffect(() => {
    const closeKeys         = getShortcut('view.closeDetail')!.keys;
    const prevDef           = getShortcut('view.prevImage')!;
    const nextDef           = getShortcut('view.nextImage')!;
    const acceptKeys        = getShortcut('inbox.accept')!.keys;
    const rejectKeys        = getShortcut('inbox.reject')!.keys;
    const deleteKeys        = getShortcut('file.delete')!.keys;
    const openDefaultKeys   = getShortcut('file.openDefault')!.keys;
    const revealKeys        = getShortcut('file.revealFolder')!.keys;
    const newWindowKeys     = getShortcut('file.newWindow')!.keys;
    const regenThumbKeys    = getShortcut('file.regenerateThumbnail')!.keys;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      const isViewingVideo = isVideoRef.current;

      // Inbox accept / reject — checked first so they take priority in inbox mode
      if (inboxModeRef.current) {
        // Accept: Enter always works; Space only for non-video (VideoPlayer handles Space for play/pause)
        if (matchesShortcut(e, acceptKeys) || (!isViewingVideo && e.key === ' ' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey)) {
          e.preventDefault();
          handleInboxAction('active');
          return;
        }
        // Reject: always works (takes priority over video.rateReset in inbox mode)
        if (matchesShortcut(e, rejectKeys)) {
          e.preventDefault();
          handleInboxAction('trash');
          return;
        }
      }

      // Close detail view — Escape always works; Enter works for both; Space only for images
      if (matchesShortcut(e, closeKeys)) {
        e.preventDefault();
        controlsRef.current!.close();
        return;
      }
      if (e.key === 'Enter' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        e.preventDefault();
        controlsRef.current!.close();
        return;
      }
      // Space closes only for images — for video, VideoPlayer handles Space for play/pause
      if (!isViewingVideo && e.key === ' ' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        e.preventDefault();
        controlsRef.current!.close();
        return;
      }

      // Rating: digit keys 0-5 without modifiers (works for both images and video)
      if (!e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        const digit = parseInt(e.key, 10);
        if (digit >= 0 && digit <= 5) {
          e.preventDefault();
          const hash = currentHashRef.current;
          if (hash) FileController.updateRating(hash, digit);
          return;
        }
      }

      // File operations — work in both image and video detail
      if (matchesShortcut(e, deleteKeys)) {
        e.preventDefault();
        const hash = currentHashRef.current;
        if (hash) {
          const inboxAction = onInboxActionRef.current;
          if (inboxAction) {
            inboxAction(hash, 'trash');
          } else {
            runCriticalAction('Delete Failed', 'detail.trashFromShortcut', api.file.setStatus(hash, 'trash'));
          }
        }
        return;
      }
      if (matchesShortcut(e, openDefaultKeys)) {
        e.preventDefault();
        const hash = currentHashRef.current;
        if (hash) {
          runCriticalAction('Open Failed', 'detail.openDefaultFromShortcut', FileController.openDefault(hash));
        }
        return;
      }
      if (matchesShortcut(e, revealKeys)) {
        e.preventDefault();
        const hash = currentHashRef.current;
        if (hash) {
          runCriticalAction('Reveal Failed', 'detail.revealFromShortcut', FileController.revealInFolder(hash));
        }
        return;
      }
      if (matchesShortcut(e, newWindowKeys)) {
        e.preventDefault();
        const img = currentImageRef.current;
        if (img) {
          runCriticalAction(
            'New Window Failed',
            'detail.openInNewWindowFromShortcut',
            FileController.openInNewWindow(img.hash, img.width, img.height),
          );
        }
        return;
      }
      if (matchesShortcut(e, regenThumbKeys)) {
        e.preventDefault();
        const hash = currentHashRef.current;
        if (hash) {
          runCriticalAction(
            'Regenerate Failed',
            'detail.regenerateThumbnailFromShortcut',
            FileController.regenerateThumbnail(hash),
          );
        }
        return;
      }

      // Navigate prev / next — arrows and A/D always navigate (both images and video)
      if (matchesShortcutDef(e, prevDef)) {
        e.preventDefault();
        controlsRef.current!.navigate(-1);
        return;
      }
      if (matchesShortcutDef(e, nextDef)) {
        e.preventDefault();
        controlsRef.current!.navigate(1);
        return;
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Navigator drag
  const handleNavigatorMouseDown = useNavigatorDrag(navigatorRef, imageSizeRef, panToNormalized);

  if (!currentImage) return null;

  const thumbUrl = mediaThumbnailUrl(currentImage.hash);

  // Keep strip visible during collection→image transition until thumbnail loads.
  // prevStripRef.current covers the first render (before layoutEffect sets holdingStrip).
  const showStrip = (stripMode && collectionImages.length > 0) || holdingStrip || (prevStripRef.current && !stripMode);

  return (
    <div className={styles.detailView}>
      {/* Backdrop: pixel-perfect freeze of previous image until new thumbnail loads */}
      {!thumbLoaded && backdropSrcRef.current && (
        <img
          src={backdropSrcRef.current}
          alt=""
          draggable={false}
          style={{
            position: 'absolute',
            inset: 0,
            width: '100%',
            height: '100%',
            objectFit: 'contain',
            pointerEvents: 'none',
            zIndex: 0,
          }}
        />
      )}
      {/* StripView — stays visible during collection→image hold */}
      {showStrip && collectionImages.length > 0 && (
        <StripView
          images={collectionImages}
          initialIndex={0}
          zoomScale={stripZoomScale}
          resetKey={stripResetKey}
          onLoadMore={stripMode && collectionHasMoreRef.current ? handleStripLoadMore : undefined}
        />
      )}
      {/* Image container — mounts hidden during strip hold so thumbnail can preload */}
      {!stripMode && (
        <div
          ref={containerRef}
          className={`${styles.imageContainer} ${shared.zoomContainer} ${isDragging ? shared.dragging : ''}`}
          onMouseDown={showStrip ? undefined : handlers.onMouseDown}
          style={showStrip ? { position: 'absolute', inset: 0, opacity: 0, pointerEvents: 'none' } : undefined}
        >
          {!isVideo ? (
            <>
              <img
                ref={thumbImgRef}
                src={displayThumbUrl || thumbUrl}
                alt=""
                draggable={false}
                onLoad={handleThumbLoad}
                style={{
                  left: '50%',
                  top: '50%',
                  width: imageSize ? imageSize.width : undefined,
                  height: imageSize ? imageSize.height : undefined,
                  opacity: thumbLoaded ? 1 : 0,
                }}
              />
              <img
                ref={imgRef}
                src={allowFullQuality ? decodedSrc : ''}
                alt=""
                decoding="async"
                onLoad={handleFullLoad}
                style={{
                  left: '50%',
                  top: '50%',
                  opacity: fullImageVisible ? 1 : 0,
                  transition: 'opacity 130ms ease',
                }}
              />
            </>
          ) : (
            <VideoPlayer
              src={mediaFileUrl(currentImage.hash, currentImage.mime)}
              autoPlay={useSettingsStore.getState().settings.videoAutoPlay}
              loop={useSettingsStore.getState().settings.videoLoop}
              muted={useSettingsStore.getState().settings.videoMuted}
              initialVolume={useSettingsStore.getState().settings.videoVolume}
              initialPlaybackRate={useSettingsStore.getState().settings.videoPlaybackRate}
              onVolumeChange={(v) => useSettingsStore.getState().updateSetting('videoVolume', v)}
              onMutedChange={(m) => useSettingsStore.getState().updateSetting('videoMuted', m)}
              onPlaybackRateChange={(r) => useSettingsStore.getState().updateSetting('videoPlaybackRate', r)}
              onLoopChange={(l) => useSettingsStore.getState().updateSetting('videoLoop', l)}
            />
          )}

          <div className={`${shared.boundaryIndicator} ${shared.left} ${boundaryFlash === 'left' ? shared.visible : ''}`}>
            First item
          </div>
          <div className={`${shared.boundaryIndicator} ${shared.right} ${boundaryFlash === 'right' && !onLoadMore ? shared.visible : ''}`}>
            Last item
          </div>

          {/* Navigator — always mounted, visibility controlled via direct DOM writes */}
          {!isVideo && (
            <div
              ref={navigatorRef}
              className={shared.navigator}
              onMouseDown={handleNavigatorMouseDown}
              style={{ display: 'none' }}
            >
              <img src={thumbUrl} alt="" draggable={false} />
              <div ref={navViewportRef} className={shared.navigatorViewport} />
            </div>
          )}
        </div>
      )}
      {/* Inbox accept/reject buttons */}
      {inboxMode && (
        <div className={styles.inboxBar}>
          <KbdTooltip label="Reject" shortcut={getShortcut('inbox.reject')!.keys} position="top">
            <button
              className={`${styles.inboxBtn} ${styles.inboxBtnReject}`}
              onClick={() => handleInboxAction('trash')}
            >
              <IconX size={18} />
            </button>
          </KbdTooltip>
          <KbdTooltip label="Accept" shortcut={getShortcut('inbox.accept')!.keys} position="top">
            <button
              className={`${styles.inboxBtn} ${styles.inboxBtnAccept}`}
              onClick={() => handleInboxAction('active')}
            >
              <IconCheck size={18} />
            </button>
          </KbdTooltip>
        </div>
      )}
    </div>
  );
}
