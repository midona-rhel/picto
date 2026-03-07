import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { api } from '#desktop/api';
import { getCurrentWindow } from '#desktop/api';
import { PhysicalSize } from '#desktop/api';
import { listen, emit } from '#desktop/api';
import { writeText } from '#desktop/api';
import {
  IconArrowsMaximize,
  IconAspectRatio,
  IconPin,
  IconPinFilled,
  IconX,
  IconRotateClockwise,
  IconFlipHorizontal,
} from '@tabler/icons-react';
import { toMasonryItem, isVideoMime, type MasonryImageItem } from './shared';
import { VideoPlayer } from '../video/VideoPlayer';
import { useSettingsStore } from '../../stores/settingsStore';
import { mediaFileUrl, mediaThumbnailUrl } from '../../lib/mediaUrl';
import { useImageZoom, type ImageSize, type ZoomState } from './useImageZoom';
import { useNavigatorDrag } from './useNavigatorDrag';
import { useNavigatorRenderer } from './useNavigatorRenderer';
import { useGlobalKeydown } from '../../hooks/useGlobalKeydown';
import { useBoundaryNavigation } from '../../hooks/useBoundaryNavigation';
import { KbdTooltip } from '../ui/KbdTooltip';
import { logBestEffortError, runBestEffort } from '../../lib/asyncOps';
import { notifyError } from '../../lib/notify';
import { useViewerMediaPipeline } from './viewer/useViewerMediaPipeline';
import styles from './DetailWindow.module.css';
import shared from './imageViewer.module.css';

interface LightImage {
  hash: string;
  name: string | null;
  mime: string;
  width: number | null;
  height: number | null;
}

interface DetailWindowProps {
  hash: string;
}

const NAV_SIZE = 120;
const TOOLBAR_HIDE_DELAY = 1000; // 1s

// Per-image zoom state cache
const zoomCache = new Map<string, ZoomState>();

export function DetailWindow({ hash }: DetailWindowProps) {
  const [image, setImage] = useState<MasonryImageItem | null>(null);
  const [images, setImages] = useState<LightImage[]>([]);
  const [totalCount, setTotalCount] = useState<number | null>(null);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const [toolbarHidden, setToolbarHidden] = useState(true); // Starts hidden
  const grayscalePreview = useSettingsStore(s => s.settings.grayscalePreview);
  const toolbarTimerRef = useRef<ReturnType<typeof setTimeout>>();

  const currentIndexRef = useRef(currentIndex);
  currentIndexRef.current = currentIndex;

  // Derive current image from list (or fallback to single loaded image)
  const currentImage = useMemo(() => {
    if (images.length > 0 && images[currentIndex]) {
      const img = images[currentIndex];
      return {
        hash: img.hash,
        name: img.name,
        mime: img.mime,
        width: img.width,
        height: img.height,
        aspectRatio: (img.width && img.height) ? img.width / img.height : 1,
      } as MasonryImageItem;
    }
    return image;
  }, [images, currentIndex, image]);

  const isVideo = currentImage ? isVideoMime(currentImage.mime) : false;

  const [rotation, setRotation] = useState<0 | 90 | 180 | 270>(0);
  const [flippedH, setFlippedH] = useState(false);

  // Reset rotation/flip when navigating to a different file
  useEffect(() => {
    setRotation(0);
    setFlippedH(false);
  }, [currentIndex]);

  const handleRotateCW = useCallback(() => {
    setRotation(prev => ((prev + 90) % 360) as 0 | 90 | 180 | 270);
  }, []);

  const handleFlipH = useCallback(() => {
    setFlippedH(prev => !prev);
  }, []);

  const contentTransform = useMemo(() => {
    const parts: string[] = [];
    if (rotation !== 0) parts.push(`rotate(${rotation}deg)`);
    if (flippedH) parts.push('scaleX(-1)');
    return parts.length > 0 ? parts.join(' ') : undefined;
  }, [rotation, flippedH]);

  // Shows on mouse move, hides after 1s of no movement
  const resetToolbarTimer = useCallback(() => {
    setToolbarHidden(false);
    clearTimeout(toolbarTimerRef.current);
    toolbarTimerRef.current = setTimeout(() => {
      setToolbarHidden(true);
    }, TOOLBAR_HIDE_DELAY);
  }, []);

  useEffect(() => {
    const onMouseMove = () => resetToolbarTimer();
    const onBlur = () => {
      clearTimeout(toolbarTimerRef.current);
      setToolbarHidden(true);
    };
    const onFocus = () => resetToolbarTimer();

    document.addEventListener('mousemove', onMouseMove);
    window.addEventListener('blur', onBlur);
    window.addEventListener('focus', onFocus);

    // Initial show then auto-hide
    resetToolbarTimer();

    return () => {
      document.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('focus', onFocus);
      clearTimeout(toolbarTimerRef.current);
    };
  }, [resetToolbarTimer]);

  useEffect(() => {
    api.file.get(hash).then((raw) => {
      if (!raw) return;
      setImage(toMasonryItem(raw));
    }).catch((err) => {
      console.error('[detail] Failed to load file:', err);
    });
  }, [hash]);

  useEffect(() => {
    let cancelled = false;

    const setup = async () => {
      const unlisten = await listen<{ images: LightImage[]; totalCount?: number | null }>('detail-images', (event) => {
        if (cancelled) return;
        const list = event.payload.images;
        if (list && list.length > 0) {
          setImages(list);
          setTotalCount(event.payload.totalCount ?? null);
          const idx = list.findIndex(i => i.hash === hash);
          if (idx >= 0) setCurrentIndex(idx);
        }
      });

      await emit('detail-window-ready', { hash });
      return unlisten;
    };

    const p = setup();
    return () => {
      cancelled = true;
      runBestEffort('detailWindow.unlistenDetailImages', p.then((fn) => fn()));
    };
  }, [hash]);

  const containerRef = useRef<HTMLDivElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const thumbImgRef = useRef<HTMLImageElement>(null);
  const minimapRef = useRef<HTMLDivElement>(null);
  const minimapVpRef = useRef<HTMLDivElement>(null);

  const imageSize: ImageSize | null = currentImage?.width && currentImage?.height
    ? { width: currentImage.width, height: currentImage.height }
    : null;

  const {
    state: zoomState,
    setState: setZoomState,
    isDragging,
    calcFitScale,
    fitToWindow,
    fitActual,
    zoomTo,
    navigatorRect,
    panToNormalized,
    handlers,
  } = useImageZoom(containerRef, imageSize);

  // Navigation-in-progress guard — prevents ResizeObserver and aspect-lock from firing during navigate
  const navigationInProgressRef = useRef(false);
  const currentAspectRef = useRef(1);

  const imageUrl = useMemo(() => {
    if (!currentImage) return '';
    return mediaFileUrl(currentImage.hash, currentImage.mime);
  }, [currentImage]);

  const thumbUrl = currentImage ? mediaThumbnailUrl(currentImage.hash) : '';

  const imageSizeRef = useRef(imageSize);
  imageSizeRef.current = imageSize;
  useNavigatorRenderer(imgRef, minimapRef, minimapVpRef, imageSizeRef, zoomState, navigatorRect, NAV_SIZE, thumbImgRef);

  const onHashChangeStart = useCallback(() => {
    navigationInProgressRef.current = true;
    if (currentImage?.width && currentImage?.height) {
      currentAspectRef.current = currentImage.width / currentImage.height;
    }
  }, [currentImage]);

  const {
    decodedSrc,
    thumbLoaded,
    fullImageVisible,
    displayThumbUrl,
    allowFullQuality,
    backdropSrcRef,
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
    onHashChangeStart,
    onImageReady: () => {
      navigationInProgressRef.current = false;
    },
    ensureThumbLogContext: 'detailWindow.ensureThumbnail',
    preloadThumbLogContext: 'detailWindow.preloadNeighborThumbnail',
  });

  // Snap to the image's aspect ratio on resize; skip during navigation
  useEffect(() => {
    let adjusting = false;
    let prevW = 0;
    let prevH = 0;

    const unlistenPromise = getCurrentWindow().onResized(async ({ payload: size }) => {
      if (adjusting || navigationInProgressRef.current) return;
      const aspect = currentAspectRef.current;
      const w = size.width;
      const h = size.height;

      if (prevW === 0) {
        prevW = w;
        prevH = h;
        return;
      }

      // Determine which dimension the user dragged
      const dw = Math.abs(w - prevW);
      const dh = Math.abs(h - prevH);

      let targetW: number;
      let targetH: number;
      if (dw >= dh) {
        targetW = w;
        targetH = Math.round(w / aspect);
      } else {
        targetH = h;
        targetW = Math.round(h * aspect);
      }

      prevW = targetW;
      prevH = targetH;

      if (Math.abs(w - targetW) > 2 || Math.abs(h - targetH) > 2) {
        adjusting = true;
        try {
          await getCurrentWindow().setSize(new PhysicalSize(targetW, targetH));
        } catch (error) {
          logBestEffortError('detailWindow.lockAspectResize', error);
        }
        adjusting = false;
      }
    });

    return () => { runBestEffort('detailWindow.unlistenResized', unlistenPromise.then((fn) => fn())); };
  }, []);

  // Keeps the same relative zoom level across window resize
  const prevContainerDimsRef = useRef({ w: 0, h: 0 });
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const ro = new ResizeObserver(() => {
      const newW = container.clientWidth;
      const newH = container.clientHeight;
      const prev = prevContainerDimsRef.current;

      // Skip proportional zoom recalc during navigation — just track dims
      if (navigationInProgressRef.current) {
        prevContainerDimsRef.current = { w: newW, h: newH };
        return;
      }

      if (prev.w > 0 && newW > 0 && imageSizeRef.current && (newW !== prev.w || newH !== prev.h)) {
        const iSize = imageSizeRef.current;
        const oldFit = Math.min(prev.w / iSize.width, prev.h / iSize.height, 1);
        const newFit = Math.min(newW / iSize.width, newH / iSize.height, 1);

        if (oldFit > 0 && newFit > 0) {
          const scaleRatio = newFit / oldFit;
          setZoomState(s => ({
            scale: s.scale * scaleRatio,
            tx: s.tx * (newW / prev.w),
            ty: s.ty * (newH / prev.h),
          }));
        }
      }

      prevContainerDimsRef.current = { w: newW, h: newH };
    });

    ro.observe(container);
    return () => ro.disconnect();
  }, [setZoomState]);

  const { navigate, boundaryFlash } = useBoundaryNavigation(images.length, setCurrentIndex, currentIndexRef);

  const toggleAlwaysOnTop = useCallback(async () => {
    const next = !alwaysOnTop;
    setAlwaysOnTop(next);
    try {
      await getCurrentWindow().setAlwaysOnTop(next);
    } catch (error) {
      setAlwaysOnTop(!next);
      notifyError(error, 'Pin Failed');
    }
  }, [alwaysOnTop]);

  const handleCopyPath = useCallback(async () => {
    if (!currentImage) return;
    try {
      const path = await api.file.resolvePath(currentImage.hash);
      await writeText(path);
    } catch (error) {
      notifyError(error, 'Copy Failed');
    }
  }, [currentImage]);

  const isVideoRef = useRef(isVideo);
  isVideoRef.current = isVideo;
  const handleDetailWindowHotkeys = useCallback((e: KeyboardEvent) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        runBestEffort('detailWindow.closeFromShortcut', getCurrentWindow().close());
        break;
      case 'ArrowLeft':
      case 'a':
        e.preventDefault();
        navigate(-1);
        break;
      case 'ArrowRight':
      case 'd':
        e.preventDefault();
        navigate(1);
        break;
      case '`':
        if (!isVideoRef.current) { e.preventDefault(); fitToWindow(); }
        break;
      case '=':
      case '+':
        if (!isVideoRef.current) { e.preventDefault(); zoomTo(zoomState.scale * 1.25); }
        break;
      case '-':
        if (!isVideoRef.current && !e.metaKey && !e.ctrlKey) {
          e.preventDefault();
          zoomTo(zoomState.scale / 1.25);
        }
        break;
      case '0':
        if (!isVideoRef.current && (e.metaKey || e.ctrlKey)) {
          e.preventDefault();
          fitActual();
        }
        break;
      case 't':
      case 'T':
        e.preventDefault();
        toggleAlwaysOnTop();
        break;
      case 'c':
        if ((e.metaKey || e.ctrlKey) && e.altKey) {
          e.preventDefault();
          handleCopyPath();
        }
        break;
    }
  }, [navigate, fitToWindow, fitActual, zoomTo, zoomState.scale, toggleAlwaysOnTop, handleCopyPath]);
  useGlobalKeydown(handleDetailWindowHotkeys);

  const handleMinimapMouseDown = useNavigatorDrag(minimapRef, imageSizeRef, panToNormalized);

  const titleText = useMemo(() => {
    if (!currentImage) return '';
    const name = currentImage.name || currentImage.hash.slice(0, 12);
    if (currentImage.width && currentImage.height) {
      return `${name} (${currentImage.width}\u00d7${currentImage.height})`;
    }
    return name;
  }, [currentImage]);

  const zoomPercent = Math.round(zoomState.scale * 100);

  return (
    <div className={styles.root}>
      {currentImage && (
        <div className={`${styles.toolbar} ${toolbarHidden ? styles.toolbarHidden : ''}`}>
          <div className={styles.toolbarLeft}>
            <span className={styles.titleName}>{titleText}</span>
            {images.length > 1 && (
              <span className={styles.counter}>
                {currentIndex + 1} / {totalCount ?? images.length}
              </span>
            )}
          </div>

          <div className={styles.toolbarRight}>
            {!isVideo && (
              <>
                <span className={styles.zoomRatio} title={`${zoomPercent}%`}>
                  {zoomPercent}%
                </span>

                <KbdTooltip label="Actual size" shortcut="Mod+0">
                  <button
                    className={styles.icBtn}
                    onClick={() => fitActual()}
                  >
                    <IconArrowsMaximize size={16} />
                  </button>
                </KbdTooltip>

                <KbdTooltip label="Fit to window" shortcut="`">
                  <button
                    className={styles.icBtn}
                    onClick={() => fitToWindow()}
                  >
                    <IconAspectRatio size={16} />
                  </button>
                </KbdTooltip>
              </>
            )}

            <button
              className={`${styles.icBtn} ${rotation !== 0 ? styles.active : ''}`}
              onClick={handleRotateCW}
              title={`Rotate (${rotation}°)`}
            >
              <IconRotateClockwise size={16} />
            </button>

            <button
              className={`${styles.icBtn} ${flippedH ? styles.active : ''}`}
              onClick={handleFlipH}
              title={flippedH ? 'Flip horizontal (on)' : 'Flip horizontal'}
            >
              <IconFlipHorizontal size={16} />
            </button>

            <KbdTooltip label={alwaysOnTop ? 'Unpin' : 'Always on top'} shortcut="T">
              <button
                className={`${styles.icBtn} ${alwaysOnTop ? styles.active : ''}`}
                onClick={toggleAlwaysOnTop}
              >
                {alwaysOnTop ? <IconPinFilled size={16} /> : <IconPin size={16} />}
              </button>
            </KbdTooltip>

            <KbdTooltip label="Close" shortcut="Escape">
              <button
                className={styles.icBtn}
                onClick={() => runBestEffort('detailWindow.closeFromButton', getCurrentWindow().close())}
              >
                <IconX size={16} />
              </button>
            </KbdTooltip>
          </div>
        </div>
      )}

      {/* Always rendered so useImageZoom attaches wheel listener */}
      <div
        ref={containerRef}
        className={`${styles.container} ${shared.zoomContainer} ${isDragging ? shared.dragging : ''}`}
        style={grayscalePreview ? { filter: 'grayscale(1)' } : undefined}
        onMouseDown={handlers.onMouseDown}
      >
        {currentImage ? (
          <>
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
            {!isVideo ? (
              <>
                {thumbUrl && (
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
                      transform: contentTransform,
                    }}
                  />
                )}
                <img
                  ref={imgRef}
                  src={allowFullQuality ? decodedSrc : ''}
                  alt=""
                  draggable={false}
                  decoding="async"
                  onLoad={handleFullLoad}
                  style={{
                    left: '50%',
                    top: '50%',
                    opacity: fullImageVisible ? 1 : 0,
                    transition: 'opacity 130ms ease',
                    transform: contentTransform,
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
                videoTransform={contentTransform}
              />
            )}

            <div className={`${shared.boundaryIndicator} ${shared.left} ${boundaryFlash === 'left' ? shared.visible : ''}`}>
              First item
            </div>
            <div className={`${shared.boundaryIndicator} ${shared.right} ${boundaryFlash === 'right' ? shared.visible : ''}`}>
              Last item
            </div>

            {!isVideo && (
              <div
                ref={minimapRef}
                className={shared.navigator}
                onMouseDown={handleMinimapMouseDown}
                style={{ display: 'none' }}
              >
                <img src={thumbUrl} alt="" draggable={false} />
                <div ref={minimapVpRef} className={shared.navigatorViewport} />
              </div>
            )}
          </>
        ) : (
          <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
            <span style={{ color: 'var(--color-text-secondary)', fontSize: 'var(--font-size-md)' }}>Loading...</span>
          </div>
        )}
      </div>
    </div>
  );
}
