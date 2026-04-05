/**
 * MediaView — inline image/video viewer.
 *
 * Image model (copied from legacy v0.5.0-alpha exactly):
 * - Frame div: position:absolute; left:50%; top:50% (zero-sized, at container center)
 * - Image: width/height = natural pixels, transform: translate(-50%, -50%)
 * - Zoom hook on frame: translate(calc(-50% + tx)px, calc(-50% + ty)px) scale(s)
 * - Container: overflow:hidden clips the result
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useSetAtom } from 'jotai';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import { mediaThumbnailUrl, mediaFileUrl } from '../../shared/lib/mediaUrl';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { viewerDisplayStateAtom, viewerDisplayControlsAtom } from '../../state/viewer';
import * as entityMutations from '../../controllers/entityMutations';
import { useImageZoom, type ImageSize } from './hooks/useImageZoom';
import { useViewerMediaPipeline } from './hooks/useViewerMediaPipeline';
import { useNavigatorRenderer } from './hooks/useNavigatorRenderer';
import { useNavigatorDrag } from './hooks/useNavigatorDrag';
import { VideoPlayer } from './video/VideoPlayer';
import styles from './MediaView.module.css';

export interface MediaViewProps {
  items: CanonicalEntityGridItem[];
  currentIndex: number;
  totalCount?: number | null;
  onNavigate: (delta: number) => void;
  onClose: (exitHash: string) => void;
  onLoadMore?: () => void;
}

const NAV_SIZE = 120;

export function MediaView({
  items, currentIndex, totalCount, onNavigate, onClose, onLoadMore,
}: MediaViewProps) {
  const currentItem = items[currentIndex] ?? null;
  const currentHash = currentItem?.entity_hash ?? '';
  const currentMime = currentItem?.mime_type ?? '';
  const isVideo = currentMime.startsWith('video/');
  const total = totalCount ?? items.length;
  const thumbHash = currentItem?.thumbnail_hash ?? currentItem?.entity_hash ?? '';

  const setDisplayState = useSetAtom(viewerDisplayStateAtom);
  const setDisplayControls = useSetAtom(viewerDisplayControlsAtom);

  // ── Refs ──
  const containerRef = useRef<HTMLDivElement>(null);
  const thumbFrameRef = useRef<HTMLDivElement>(null);
  const fullFrameRef = useRef<HTMLDivElement>(null);
  const fullImgRef = useRef<HTMLImageElement>(null);
  const navigatorRef = useRef<HTMLDivElement>(null);
  const navViewportRef = useRef<HTMLDivElement>(null);

  // ── Media pipeline (must be before imageSize since we derive from displayedHash) ──
  const neighborHashes = useMemo(() => {
    const r: string[] = [];
    const prev = items[currentIndex - 1];
    const next = items[currentIndex + 1];
    if (prev) r.push(prev.thumbnail_hash ?? prev.entity_hash);
    if (next) r.push(next.thumbnail_hash ?? next.entity_hash);
    return r;
  }, [items, currentIndex]);

  const pipeline = useViewerMediaPipeline({
    hash: currentHash || null,
    thumbnailHash: currentItem?.thumbnail_hash ?? null,
    mime: currentMime,
    isVideo,
    imgRef: fullImgRef,
    neighborHashes,
  });

  // ── Image size — derived from the DISPLAYED item, not the requested one ──
  const displayedItem = pipeline.displayedHash
    ? items.find((it) => it.entity_hash === pipeline.displayedHash) ?? currentItem
    : currentItem;
  const imageSize = useMemo<ImageSize | null>(() => {
    if (!displayedItem?.pixel_width || !displayedItem?.pixel_height) return null;
    return { width: displayedItem.pixel_width, height: displayedItem.pixel_height };
  }, [pipeline.displayedHash]); // eslint-disable-line react-hooks/exhaustive-deps

  const imageSizeRef = useRef(imageSize);
  imageSizeRef.current = imageSize;

  // ── Zoom/pan ──
  const zoom = useImageZoom(containerRef, imageSize, [thumbFrameRef, fullFrameRef]);

  // Fit image when displayed hash changes (not when requested hash changes).
  useLayoutEffect(() => {
    if (!imageSize) return;
    const el = containerRef.current;
    if (!el) return;
    const cw = el.clientWidth;
    const ch = el.clientHeight;
    if (cw === 0 || ch === 0) return;
    const fitScale = Math.min(cw / imageSize.width, ch / imageSize.height);
    zoom.setState({ scale: fitScale, tx: 0, ty: 0 });
  }, [pipeline.displayedHash, imageSize]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Navigator ──
  useNavigatorRenderer(navigatorRef, navViewportRef, imageSizeRef, zoom.navigatorRect, NAV_SIZE, zoom.onLiveFrameRef, zoom.containerSize);
  const handleNavMouseDown = useNavigatorDrag(navigatorRef, imageSizeRef, zoom.panToNormalized);

  // ── Boundary flash ──
  const [boundaryFlash, setBoundaryFlash] = useState<'left' | 'right' | null>(null);
  const boundaryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const navigate = useCallback((delta: number) => {
    const nextIdx = currentIndex + delta;
    if (nextIdx < 0 || nextIdx >= items.length) {
      setBoundaryFlash(nextIdx < 0 ? 'left' : 'right');
      if (boundaryTimerRef.current) clearTimeout(boundaryTimerRef.current);
      boundaryTimerRef.current = setTimeout(() => setBoundaryFlash(null), 800);
      if (nextIdx >= items.length && onLoadMore) onLoadMore();
      return;
    }
    setBoundaryFlash(null);
    onNavigate(delta);
  }, [currentIndex, items.length, onNavigate, onLoadMore]);

  // ── Toolbar state ──
  const zoomPercent = Math.round(zoom.state.scale * 100);

  useEffect(() => {
    setDisplayState({ currentIndex, total, zoomPercent });
  }, [currentIndex, total, zoomPercent, setDisplayState]);

  // Zoom % updates only on committed state (after 96ms debounce).
  // Live per-frame updates removed — settle-only is sufficient.

  useEffect(() => {
    setDisplayControls({
      close: () => onClose(currentHash),
      navigate,
      fitToWindow: zoom.fitToWindow,
      fitActual: zoom.fitActual,
      zoomIn: () => zoom.animateZoomTo(zoom.state.scale * 1.25),
      zoomOut: () => zoom.animateZoomTo(zoom.state.scale / 1.25),
      setZoomScale: (s) => zoom.zoomTo(s),
    });
  }, [currentHash, navigate, onClose, zoom, setDisplayControls]);

  useEffect(() => () => { setDisplayState(null); setDisplayControls(null); }, [setDisplayState, setDisplayControls]);

  // ── Keyboard — uses registry defs so EU keys2 alternatives work ──
  useEffect(() => {
    const closeDef = getShortcut('view.closeDetail')!;
    const detailDef = getShortcut('view.detailView')!;
    const quicklookDef = getShortcut('view.quicklook')!;
    const prevDef = getShortcut('view.prevImage')!;
    const nextDef = getShortcut('view.nextImage')!;
    const fitDef = getShortcut('view.fitWindow')!;
    const zoomInDef = getShortcut('view.zoomIn')!;
    const zoomOutDef = getShortcut('view.zoomOut')!;
    const actualDef = getShortcut('view.actualSize')!;

    const handleKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      if (matchesShortcutDef(e, closeDef) || matchesShortcutDef(e, detailDef) || matchesShortcutDef(e, quicklookDef)) { e.preventDefault(); onClose(currentHash); return; }
      if (matchesShortcutDef(e, prevDef)) { e.preventDefault(); navigate(-1); return; }
      if (matchesShortcutDef(e, nextDef)) { e.preventDefault(); navigate(1); return; }
      if (matchesShortcutDef(e, fitDef)) { e.preventDefault(); zoom.fitToWindow(); return; }
      if (matchesShortcutDef(e, zoomInDef)) { e.preventDefault(); zoom.animateZoomTo(zoom.state.scale * 1.25); return; }
      if (matchesShortcutDef(e, zoomOutDef)) { e.preventDefault(); zoom.animateZoomTo(zoom.state.scale / 1.25); return; }
      if (matchesShortcutDef(e, actualDef)) { e.preventDefault(); zoom.fitActual(); return; }

      // Rating: 0-5 (no modifiers)
      if (!e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && e.key >= '0' && e.key <= '5') {
        e.preventDefault();
        void entityMutations.setEntityRating(currentHash, parseInt(e.key, 10));
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [currentHash, navigate, onClose, zoom]);

  useEffect(() => () => { if (boundaryTimerRef.current) clearTimeout(boundaryTimerRef.current); }, []);

  if (!currentItem) return null;

  const thumbUrl = mediaThumbnailUrl(thumbHash);

  return (
    <div className={styles.mediaView}>
      {isVideo ? (
        <VideoPlayer key={currentHash} src={mediaFileUrl(thumbHash, currentMime)} />
      ) : (
        <div ref={containerRef} className={`${styles.zoomContainer} ${zoom.isDragging ? styles.dragging : ''}`}
          onMouseDown={zoom.handlers.onMouseDown}>

          {/* Thumbnail frame — at center, zoom hook applies translate(calc(-50% + tx) scale(s)) */}
          <div ref={thumbFrameRef} style={{ position: 'absolute', left: '50%', top: '50%' }}>
            <img
              src={pipeline.thumbUrl || thumbUrl}
              alt="" draggable={false}
              onLoad={pipeline.handleThumbLoad}
              style={{
                display: 'block',
                width: imageSize?.width,
                height: imageSize?.height,
                opacity: pipeline.thumbLoaded ? 1 : 0,
              }}
            />
          </div>

          {/* Full-res frame — same position, fades in */}
          <div ref={fullFrameRef} style={{ position: 'absolute', left: '50%', top: '50%' }}>
            {pipeline.fullUrl && (
              <img
                ref={fullImgRef}
                src={pipeline.fullUrl}
                alt="" decoding="async" draggable={false}
                onLoad={pipeline.handleFullLoad}
                style={{
                  display: 'block',
                  width: imageSize?.width,
                  height: imageSize?.height,
                  opacity: pipeline.fullVisible ? 1 : 0,
                  transition: 'opacity 130ms ease',
                }}
              />
            )}
          </div>

          {/* Boundary flash */}
          <div className={`${styles.boundaryLeft} ${boundaryFlash === 'left' ? styles.boundaryVisible : ''}`}>First item</div>
          <div className={`${styles.boundaryRight} ${boundaryFlash === 'right' ? styles.boundaryVisible : ''}`}>Last item</div>

          {/* Navigator — always mounted, visibility via useNavigatorRenderer */}
          <div ref={navigatorRef} className={styles.navigator} onMouseDown={handleNavMouseDown} style={{ display: 'none' }}>
            <img src={thumbUrl} alt="" draggable={false} className={styles.navigatorThumb} />
            <div ref={navViewportRef} className={styles.navigatorViewport} />
          </div>
        </div>
      )}
    </div>
  );
}
