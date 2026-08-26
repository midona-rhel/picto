/**
 * DetailWindow — standalone image/video viewer in a separate Electron window.
 *
 * Differences from inline MediaView:
 * - Own window with auto-hiding toolbar
 * - Navigation via IPC (receives image list from main window)
 * - Always-on-top pin toggle
 * - Proportional zoom on window resize (image stays fit)
 * - No aspect ratio lock — window resizes freely, theme bg fills gaps
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import {
  IconPin,
  IconPinFilled,
} from '@tabler/icons-react';
import { mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ToolbarActualSizeIcon, ToolbarCloseIcon, ToolbarFitIcon } from '../../shared/ui/icons/toolbar-icons';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { useImageZoom, type ImageSize, type ZoomState } from './hooks/useImageZoom';
import { useMediaImagePipeline } from '../../shared/hooks/useMediaImagePipeline';
import { useRecordMediaView } from './hooks/useRecordMediaView';
import { useNavigatorRenderer } from './hooks/useNavigatorRenderer';
import { useNavigatorDrag } from './hooks/useNavigatorDrag';
import { DetailMediaRenderer } from './document/DetailMediaRenderer';
import { detailRendererKind } from './document/detailRendererKind';
import { useViewerEntityContextMenu } from './useViewerEntityContextMenu';
import type { FlashPlaybackController } from './document/FlashPlayer';
import { FlashControls } from './document/FlashControls';
import type { CurrentFrameCapture } from './currentFrameCapture';
import type { ViewerZoomControls } from '../../state/viewer';
import { filesController } from '../../controllers/filesController';
import { windowController } from '../../controllers/windowController';
import { ImageCrossfadeFrame } from './ImageCrossfadeFrame';
import { usePreviewPreferences } from './usePreviewPreferences';
import { LibraryCoverDialogHost } from '../library/LibraryCoverDialogHost';
import styles from './DetailWindow.module.css';
import viewerStyles from './MediaView.module.css';

// ── Types ────────────────────────────────────────────────────────

interface LightImage {
  item_id: number;
  hash: string;
  name: string | null;
  mime: string;
  width: number | null;
  height: number | null;
}

interface DetailWindowProps {
  hash: string;
}

declare const window: Window & {
  picto: {
    api: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> };
    events: {
      on: (name: string, handler: (payload: unknown) => void) => Promise<() => void>;
      emit: (name: string, payload: unknown) => Promise<void>;
    };
    clipboard: { writeText: (text: string) => Promise<void> };
  };
};

// ── Constants ────────────────────────────────────────────────────

const NAV_SIZE = 120;
const TOOLBAR_HIDE_DELAY = 1000;

// Per-image zoom state cache
const zoomCache = new Map<string, ZoomState>();

// ── Component ────────────────────────────────────────────────────

export function DetailWindow({ hash }: DetailWindowProps) {
  const previewPreferences = usePreviewPreferences();
  const [images, setImages] = useState<LightImage[]>([]);
  const [totalCount, setTotalCount] = useState<number | null>(null);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const [toolbarHidden, setToolbarHidden] = useState(true);
  const [flashPlayback, setFlashPlayback] = useState<FlashPlaybackController | null>(null);
  const [captureCurrentFrame, setCaptureCurrentFrame] = useState<CurrentFrameCapture | null>(null);
  const [pdfZoomControls, setPdfZoomControls] = useState<ViewerZoomControls | null>(null);
  const [pdfZoomPercent, setPdfZoomPercent] = useState(100);
  const handleFrameCaptureChange = useCallback((capture: CurrentFrameCapture | null) => {
    setCaptureCurrentFrame(capture ? () => capture : null);
  }, []);
  const toolbarTimerRef = useRef<ReturnType<typeof setTimeout>>();
  const currentIndexRef = useRef(currentIndex);
  currentIndexRef.current = currentIndex;

  // ── IPC: receive image list from main window ──
  useEffect(() => {
    let cancelled = false;
    const setup = async () => {
      const unlisten = await window.picto.events.on('detail-images', (payload: unknown) => {
        if (cancelled) return;
        const data = payload as { images?: LightImage[]; totalCount?: number | null };
        if (data.images?.length) {
          setImages(data.images);
          setTotalCount(data.totalCount ?? null);
          const idx = data.images.findIndex((i) => i.hash === hash);
          if (idx >= 0) setCurrentIndex(idx);
        }
      });
      void window.picto.events.emit('detail-window-ready', { hash });
      return unlisten;
    };
    const p = setup();
    return () => {
      cancelled = true;
      p.then((fn) => fn()).catch(() => {});
    };
  }, [hash]);

  // ── Current image ──
  const currentImage = useMemo(() => {
    if (images.length > 0 && images[currentIndex]) return images[currentIndex];
    return null;
  }, [images, currentIndex]);
  useRecordMediaView(currentImage?.item_id);

  const rendererKind = detailRendererKind(currentImage?.mime ?? '');
  const isImage = rendererKind === 'image';
  const usesRendererZoom = rendererKind === 'pdf' || rendererKind === 'jpeg-xl';
  const supportsZoom = isImage || (usesRendererZoom && pdfZoomControls != null);
  const contextMenu = useViewerEntityContextMenu({
    hash: currentImage?.hash ?? null,
    name: currentImage?.name,
    mime: currentImage?.mime,
    width: currentImage?.width,
    height: currentImage?.height,
    flashPlayback: rendererKind === 'flash' ? flashPlayback : null,
    captureCurrentFrame,
  });
  const thumbHash = currentImage?.hash ?? hash;

  // ── Toolbar auto-hide ──
  const resetToolbarTimer = useCallback(() => {
    setToolbarHidden(false);
    clearTimeout(toolbarTimerRef.current);
    toolbarTimerRef.current = setTimeout(() => setToolbarHidden(true), TOOLBAR_HIDE_DELAY);
  }, []);

  useEffect(() => {
    const onMove = () => resetToolbarTimer();
    const onBlur = () => { clearTimeout(toolbarTimerRef.current); setToolbarHidden(true); };
    const onFocus = () => resetToolbarTimer();
    // Keep toolbar visible while dragging the window (Electron sends this during drag)
    const onWindowMoved = () => resetToolbarTimer();
    document.addEventListener('mousemove', onMove);
    window.addEventListener('blur', onBlur);
    window.addEventListener('focus', onFocus);
    const picto = (window as any).picto;
    let unlistenWindowMoved: (() => void) | null = null;
    if (picto?.events?.on) {
      picto.events.on('picto:window-moved', onWindowMoved).then((fn: () => void) => {
        unlistenWindowMoved = fn;
      });
    }
    resetToolbarTimer();
    return () => {
      document.removeEventListener('mousemove', onMove);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('focus', onFocus);
      unlistenWindowMoved?.();
      clearTimeout(toolbarTimerRef.current);
    };
  }, [resetToolbarTimer]);

  // ── Refs ──
  const containerRef = useRef<HTMLDivElement>(null);
  const imageFrameRef = useRef<HTMLDivElement>(null);
  const fullImgRef = useRef<HTMLImageElement>(null);
  const navigatorRef = useRef<HTMLDivElement>(null);
  const navViewportRef = useRef<HTMLDivElement>(null);

  // ── Image size ──
  const imageSize = useMemo<ImageSize | null>(() => {
    if (!currentImage?.width || !currentImage?.height) return null;
    return { width: currentImage.width, height: currentImage.height };
  }, [currentImage?.width, currentImage?.height]);

  const imageSizeRef = useRef(imageSize);
  imageSizeRef.current = imageSize;

  // ── Zoom/pan ──
  const zoom = useImageZoom(containerRef, imageSize, [imageFrameRef], {
    macTrackpadGestures: previewPreferences.viewerTrackpadGestures,
  });

  // ── Media pipeline ──
  const neighborHashes = useMemo(() => {
    const r: string[] = [];
    const prev = images[currentIndex - 1];
    const next = images[currentIndex + 1];
    if (prev) r.push(prev.hash);
    if (next) r.push(next.hash);
    return r;
  }, [images, currentIndex]);

  const pipeline = useMediaImagePipeline({
    hash: currentImage?.hash ?? null,
    thumbnailHash: currentImage?.hash ?? null,
    mime: currentImage?.mime ?? '',
    isVideo: !isImage,
    neighborHashes,
  });

  // Fit image to window as soon as we know the image dimensions.
  // Track which hash we last fitted so we only fit once per image.
  const lastFittedHashRef = useRef<string | null>(null);
  useEffect(() => {
    const h = currentImage?.hash ?? null;
    if (!h || !imageSize) return;
    if (lastFittedHashRef.current === h) return;
    lastFittedHashRef.current = h;

    const el = containerRef.current;
    if (!el) return;
    const cw = el.clientWidth;
    const ch = el.clientHeight;
    if (cw === 0 || ch === 0) return;

    // Restore cached zoom or fit
    const cached = zoomCache.get(h);
    if (cached) {
      zoom.setState(cached);
    } else {
      const scale = previewPreferences.imageDefaultZoom === 'actual'
        ? 1
        : Math.min(cw / imageSize.width, ch / imageSize.height);
      zoom.setState({ scale, tx: 0, ty: 0 });
    }
  }, [currentImage?.hash, imageSize, previewPreferences.imageDefaultZoom]); // eslint-disable-line react-hooks/exhaustive-deps

  // Cache zoom state on change
  useEffect(() => {
    if (currentImage?.hash) zoomCache.set(currentImage.hash, zoom.state);
  }, [zoom.state, currentImage?.hash]);

  // ── Proportional zoom on container resize ──
  const prevContainerDimsRef = useRef({ w: 0, h: 0 });
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const ro = new ResizeObserver(() => {
      const newW = container.clientWidth;
      const newH = container.clientHeight;
      const prev = prevContainerDimsRef.current;
      if (prev.w > 0 && newW > 0 && imageSizeRef.current && (newW !== prev.w || newH !== prev.h)) {
        const iSize = imageSizeRef.current;
        const oldFit = Math.min(prev.w / iSize.width, prev.h / iSize.height, 1);
        const newFit = Math.min(newW / iSize.width, newH / iSize.height, 1);
        if (oldFit > 0 && newFit > 0) {
          const scaleRatio = newFit / oldFit;
          zoom.setState((s: ZoomState) => ({
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
  }, [zoom.setState]);

  // ── Navigator ──
  useNavigatorRenderer(navigatorRef, navViewportRef, imageSizeRef, zoom.navigatorRect, NAV_SIZE, zoom.onLiveFrameRef, zoom.containerSize);
  const handleNavMouseDown = useNavigatorDrag(navigatorRef, imageSizeRef, zoom.panToNormalized);

  // ── Navigation ──
  const [boundaryFlash, setBoundaryFlash] = useState<'left' | 'right' | null>(null);
  const boundaryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const navigate = useCallback((delta: number) => {
    const nextIdx = currentIndexRef.current + delta;
    if (nextIdx < 0 || nextIdx >= images.length) {
      setBoundaryFlash(nextIdx < 0 ? 'left' : 'right');
      if (boundaryTimerRef.current) clearTimeout(boundaryTimerRef.current);
      boundaryTimerRef.current = setTimeout(() => setBoundaryFlash(null), 800);
      return;
    }
    setBoundaryFlash(null);
    setCurrentIndex(nextIdx);
  }, [images.length]);

  // ── Always on top ──
  const toggleAlwaysOnTop = useCallback(() => {
    const next = !alwaysOnTop;
    setAlwaysOnTop(next);
    windowController.setCurrentWindowAlwaysOnTop(next).catch(() => setAlwaysOnTop(!next));
  }, [alwaysOnTop]);

  // ── Copy path ──
  const handleCopyPath = useCallback(async () => {
    if (!currentImage) return;
    try {
      const path = await filesController.resolveFilePath(currentImage.hash);
      if (!path) return;
      await window.picto.clipboard.writeText(path);
    } catch {}
  }, [currentImage]);

  // ── Keyboard shortcuts (with EU alternatives via shortcut registry) ──
  useShortcutScope((e) => {
    const prevDef = getShortcut('view.prevImage');
    const nextDef = getShortcut('view.nextImage');
    const fitDef = getShortcut('view.fitWindow');
    const zoomInDef = getShortcut('view.zoomIn');
    const zoomOutDef = getShortcut('view.zoomOut');
    const actualDef = getShortcut('view.actualSize');
    const closeDef = getShortcut('view.closeDetail');
    const copyPathDef = getShortcut('edit.copyFilePath');
    const copyDef = getShortcut('edit.copy');

      // Close window
      if (closeDef && matchesShortcutDef(e, closeDef)) {
        e.preventDefault();
        void windowController.closeCurrentWindow();
        return;
      }

      // Navigation
      if (prevDef && matchesShortcutDef(e, prevDef)) { e.preventDefault(); navigate(-1); return; }
      if (nextDef && matchesShortcutDef(e, nextDef)) { e.preventDefault(); navigate(1); return; }

      // Zoom
      const activeZoom = usesRendererZoom ? pdfZoomControls : isImage ? {
        fitToWindow: zoom.fitToWindow,
        fitActual: zoom.fitActual,
        zoomIn: () => zoom.animateZoomTo(zoom.state.scale * 1.25),
        zoomOut: () => zoom.animateZoomTo(zoom.state.scale / 1.25),
      } : null;
      if (activeZoom) {
        if (fitDef && matchesShortcutDef(e, fitDef)) { e.preventDefault(); activeZoom.fitToWindow(); return; }
        if (zoomInDef && matchesShortcutDef(e, zoomInDef)) { e.preventDefault(); activeZoom.zoomIn(); return; }
        if (zoomOutDef && matchesShortcutDef(e, zoomOutDef)) { e.preventDefault(); activeZoom.zoomOut(); return; }
        if (actualDef && matchesShortcutDef(e, actualDef)) { e.preventDefault(); activeZoom.fitActual(); return; }
      }

      // Always on top
      const aotDef = getShortcut('view.alwaysOnTop');
      if (aotDef && matchesShortcutDef(e, aotDef)) {
        e.preventDefault();
        toggleAlwaysOnTop();
        return;
      }

      // Copy path
      if (copyPathDef && matchesShortcutDef(e, copyPathDef)) {
        e.preventDefault();
        handleCopyPath();
        return;
      }
      if (copyDef && matchesShortcutDef(e, copyDef) && currentImage) {
        e.preventDefault();
        void filesController.copyFileForHash(currentImage.hash);
      }
  }, { priority: 60 });

  useEffect(() => () => { if (boundaryTimerRef.current) clearTimeout(boundaryTimerRef.current); }, []);

  // ── Derived ──
  const titleText = useMemo(() => {
    if (!currentImage) return '';
    const name = currentImage.name || currentImage.hash.slice(0, 12);
    if (currentImage.width && currentImage.height) {
      return `${name} (${currentImage.width}\u00d7${currentImage.height})`;
    }
    return name;
  }, [currentImage]);

  const zoomPercent = usesRendererZoom ? pdfZoomPercent : Math.round(zoom.state.scale * 100);
  const fitActiveViewer = usesRendererZoom ? pdfZoomControls?.fitToWindow : zoom.fitToWindow;
  const actualActiveViewer = usesRendererZoom ? pdfZoomControls?.fitActual : zoom.fitActual;
  const thumbUrl = mediaThumbnailUrl(thumbHash);

  // ── Render ──
  return (
    <div
      className={styles.root}
      onContextMenuCapture={(event) => {
        if (!(event.target as Element).closest('[data-flash-player]')) contextMenu.open(event);
      }}
    >
      {currentImage && (
        <div
          className={`${styles.toolbar} ${toolbarHidden ? styles.toolbarHidden : ''}`}
          data-window-drag-region=""
        >
          <div className={styles.toolbarLeft}>
            <span className={styles.titleName}>{titleText}</span>
            {images.length > 1 && (
              <span className={styles.counter}>
                {currentIndex + 1} / {totalCount ?? images.length}
              </span>
            )}
          </div>

          <div className={styles.toolbarRight}>
            {supportsZoom && (usesRendererZoom || pipeline.thumbLoaded) && (
              <>
                <span className={styles.zoomRatio}>{zoomPercent}%</span>
                <KbdTooltip label="Actual size" shortcutId="view.actualSize">
                  <button className={styles.icBtn} onClick={actualActiveViewer}>
                    <ToolbarActualSizeIcon />
                  </button>
                </KbdTooltip>
                <KbdTooltip label="Fit to window" shortcutId="view.fitWindow">
                  <button className={styles.icBtn} onClick={fitActiveViewer}>
                    <ToolbarFitIcon />
                  </button>
                </KbdTooltip>
              </>
            )}

            <KbdTooltip label={alwaysOnTop ? 'Unpin' : 'Always on top'} shortcutId="view.alwaysOnTop">
              <button
                className={`${styles.icBtn} ${alwaysOnTop ? styles.icBtnActive : ''}`}
                onClick={toggleAlwaysOnTop}
              >
                {alwaysOnTop ? <IconPinFilled size={16} /> : <IconPin size={16} />}
              </button>
            </KbdTooltip>

            <KbdTooltip label="Close" shortcutId="view.closeDetail">
              <button
                className={styles.icBtn}
                onClick={() => void windowController.closeCurrentWindow()}
              >
                <ToolbarCloseIcon />
              </button>
            </KbdTooltip>
          </div>
        </div>
      )}

      {rendererKind === 'flash' && currentImage ? (
        <div className={styles.container}>
          <DetailMediaRenderer
            hash={currentImage.hash}
            mimeType={currentImage.mime}
            displayName={currentImage.name}
            onFlashPlaybackChange={setFlashPlayback}
            onFlashContextMenu={contextMenu.open}
            onFrameCaptureChange={handleFrameCaptureChange}
          />
          <FlashControls controller={flashPlayback} />
        </div>
      ) : !isImage && currentImage ? (
        <div className={styles.container}>
          <DetailMediaRenderer
            hash={currentImage.hash}
            mimeType={currentImage.mime}
            displayName={currentImage.name}
            onFlashPlaybackChange={setFlashPlayback}
            onFlashContextMenu={contextMenu.open}
            onFrameCaptureChange={handleFrameCaptureChange}
            onPdfZoomControlsChange={setPdfZoomControls}
            onPdfZoomPercentChange={setPdfZoomPercent}
            mediaAutoPlay={rendererKind === 'video' ? previewPreferences.videoAutoPlay : undefined}
            mediaLoop={rendererKind === 'video' ? previewPreferences.videoLoop : undefined}
          />
        </div>
      ) : (
        <div
          ref={containerRef}
          className={`${styles.container} ${zoom.isDragging ? styles.dragging : ''}`}
          onMouseDown={zoom.handlers.onMouseDown}
        >
          {currentImage ? (
            <>
              <ImageCrossfadeFrame
                frameRef={imageFrameRef}
                fullImageRef={fullImgRef}
                imageSize={imageSize}
                thumbnailUrl={pipeline.thumbUrl || thumbUrl}
                fullUrl={pipeline.fullUrl}
                thumbnailVisible={pipeline.thumbLoaded}
                fullVisible={pipeline.fullVisible}
                imageRendering={previewPreferences.imageRendering}
                showTransparencyGrid={previewPreferences.showTransparencyGrid}
                onThumbnailLoad={pipeline.handleThumbLoad}
                onFullLoad={pipeline.handleFullLoad}
              />

              {/* Boundary flash */}
              <div className={`${viewerStyles.boundaryLeft} ${boundaryFlash === 'left' ? viewerStyles.boundaryVisible : ''}`}>First item</div>
              <div className={`${viewerStyles.boundaryRight} ${boundaryFlash === 'right' ? viewerStyles.boundaryVisible : ''}`}>Last item</div>

              {/* Navigator minimap */}
              {isImage && (
                <div ref={navigatorRef} className={viewerStyles.navigator} onMouseDown={handleNavMouseDown} style={{ display: 'none' }}>
                  <img src={thumbUrl} alt="" draggable={false} className={viewerStyles.navigatorThumb} />
                  <div ref={navViewportRef} className={viewerStyles.navigatorViewport} />
                </div>
              )}
            </>
          ) : (
            <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <span style={{ color: 'var(--color-text-secondary)', fontSize: 13 }}>Loading...</span>
            </div>
          )}
        </div>
      )}
      {contextMenu.menu}
      <LibraryCoverDialogHost />
    </div>
  );
}
