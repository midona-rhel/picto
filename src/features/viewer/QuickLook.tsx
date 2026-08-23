/**
 * QuickLook — peek overlay (Space key).
 *
 * Standalone image viewer with minimal chrome. Same zoom/image model as MediaView
 * (frame at left:50% top:50%, zoom hook applies translate(calc(-50% + tx)) scale(s)).
 * No navigator minimap. Fade-in on open. Exit button top-right, nav toolbar bottom.
 */

import { useState, useEffect, useLayoutEffect, useMemo, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ToolbarChevronIcon, ToolbarCloseIcon } from '../../shared/ui/icons/toolbar-icons';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import { mediaThumbnailUrl, mediaFileUrl } from '../../shared/lib/mediaUrl';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import * as entityMutations from '../../controllers/entityMutations';
import { useImageZoom, type ImageSize } from './hooks/useImageZoom';
import { useMediaImagePipeline } from '../../shared/hooks/useMediaImagePipeline';
import { useRecordMediaView } from './hooks/useRecordMediaView';
import { VideoPlayer } from './video/VideoPlayer';
import styles from './QuickLook.module.css';

export interface QuickLookProps {
  items: CanonicalEntityGridItem[];
  currentIndex: number;
  totalCount?: number | null;
  onNavigate: (delta: number) => void;
  onClose: (exitHash: string) => void;
  onLoadMore?: () => void;
}

export function QuickLook({
  items, currentIndex, totalCount, onNavigate, onClose, onLoadMore,
}: QuickLookProps) {
  const currentItem = items[currentIndex] ?? null;
  const currentHash = currentItem?.entity_hash ?? '';
  useRecordMediaView(currentHash);
  const currentMime = currentItem?.mime_type ?? '';
  const isVideo = currentMime.startsWith('video/');
  const total = totalCount ?? items.length;
  const thumbHash = currentItem?.entity_hash ?? '';

  // Fade-in
  const [isOpen, setIsOpen] = useState(false);
  useEffect(() => {
    const raf = requestAnimationFrame(() => setIsOpen(true));
    return () => cancelAnimationFrame(raf);
  }, []);

  // Refs
  const containerRef = useRef<HTMLDivElement>(null);
  const thumbFrameRef = useRef<HTMLDivElement>(null);
  const fullFrameRef = useRef<HTMLDivElement>(null);
  const fullImgRef = useRef<HTMLImageElement>(null);

  // Media pipeline (before imageSize — derives from displayedHash)
  const neighborHashes = useMemo(() => {
    const r: string[] = [];
    const prev = items[currentIndex - 1];
    const next = items[currentIndex + 1];
    if (prev) r.push(prev.entity_hash);
    if (next) r.push(next.entity_hash);
    return r;
  }, [items, currentIndex]);

  const pipeline = useMediaImagePipeline({
    hash: currentHash || null,
    thumbnailHash: currentItem?.entity_hash ?? null,
    mime: currentMime,
    isVideo,
    imgRef: fullImgRef,
    neighborHashes,
  });

  // Image size from displayed item
  const displayedItem = pipeline.displayedHash
    ? items.find((it) => it.entity_hash === pipeline.displayedHash) ?? currentItem
    : currentItem;
  const imageSize = useMemo<ImageSize | null>(() => {
    if (!displayedItem?.pixel_width || !displayedItem?.pixel_height) return null;
    return { width: displayedItem.pixel_width, height: displayedItem.pixel_height };
  }, [pipeline.displayedHash]); // eslint-disable-line react-hooks/exhaustive-deps

  // Zoom/pan
  const zoom = useImageZoom(containerRef, imageSize, [thumbFrameRef, fullFrameRef]);

  // Fit when displayed image changes
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

  const navigate = useCallback((delta: number) => {
    const nextIdx = currentIndex + delta;
    if (nextIdx < 0 || nextIdx >= items.length) {
      if (nextIdx >= items.length && onLoadMore) onLoadMore();
      return;
    }
    onNavigate(delta);
  }, [currentIndex, items.length, onNavigate, onLoadMore]);

  // Keyboard — registry defs for EU alternative keys
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

      // Escape, Space (quicklook toggle), or Enter (detail toggle) all close
      if (matchesShortcutDef(e, closeDef) || matchesShortcutDef(e, quicklookDef) || matchesShortcutDef(e, detailDef)) { e.preventDefault(); onClose(currentHash); return; }
      if (matchesShortcutDef(e, prevDef)) { e.preventDefault(); navigate(-1); return; }
      if (matchesShortcutDef(e, nextDef)) { e.preventDefault(); navigate(1); return; }
      if (matchesShortcutDef(e, fitDef)) { e.preventDefault(); zoom.fitToWindow(); return; }
      if (matchesShortcutDef(e, zoomInDef)) { e.preventDefault(); zoom.animateZoomTo(zoom.state.scale * 1.25); return; }
      if (matchesShortcutDef(e, zoomOutDef)) { e.preventDefault(); zoom.animateZoomTo(zoom.state.scale / 1.25); return; }
      if (matchesShortcutDef(e, actualDef)) { e.preventDefault(); zoom.fitActual(); return; }

      // Rating: 0-5
      if (!e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && e.key >= '0' && e.key <= '5') {
        e.preventDefault();
        void entityMutations.setEntityRating(currentHash, parseInt(e.key, 10));
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [currentHash, navigate, onClose, zoom]);

  if (!currentItem) return null;

  const thumbUrl = mediaThumbnailUrl(thumbHash);
  const canPrev = currentIndex > 0;
  const canNext = currentIndex < items.length - 1;

  return createPortal(
    <div className={styles.overlay} data-quick-look-overlay>
      <KbdTooltip label="Close" shortcut="Space" position="bottom">
        <button className={styles.exitBtn} onClick={() => onClose(currentHash)}>
          <ToolbarCloseIcon />
        </button>
      </KbdTooltip>

      <div
        ref={containerRef}
        className={`${styles.imageArea} ${isOpen ? styles.open : ''} ${zoom.isDragging ? styles.dragging : ''}`}
        onMouseDown={zoom.handlers.onMouseDown}
      >
        {isVideo ? (
          <VideoPlayer key={currentHash} src={mediaFileUrl(thumbHash, currentMime)} />
        ) : (
          <>
            <div ref={thumbFrameRef} style={{ position: 'absolute', left: '50%', top: '50%' }}>
              <img
                src={pipeline.thumbUrl || thumbUrl}
                alt="" draggable={false}
                onLoad={pipeline.handleThumbLoad}
                style={{ display: 'block', width: imageSize?.width, height: imageSize?.height, opacity: pipeline.thumbLoaded ? 1 : 0 }}
              />
            </div>
            <div ref={fullFrameRef} style={{ position: 'absolute', left: '50%', top: '50%' }}>
              {pipeline.fullUrl && (
                <img
                  ref={fullImgRef}
                  src={pipeline.fullUrl}
                  alt="" decoding="async" draggable={false}
                  onLoad={pipeline.handleFullLoad}
                  style={{ display: 'block', width: imageSize?.width, height: imageSize?.height, opacity: pipeline.fullVisible ? 1 : 0, transition: 'opacity 130ms ease' }}
                />
              )}
            </div>
          </>
        )}
      </div>

      <div className={styles.inlineToolbar}>
        <KbdTooltip label="Previous" shortcut="ArrowLeft">
          <button className={styles.navBtn} onClick={() => navigate(-1)} disabled={!canPrev}>
            <ToolbarChevronIcon direction="left" />
          </button>
        </KbdTooltip>
        <span className={styles.pageCounter}>{currentIndex + 1} / {total}</span>
        <KbdTooltip label="Next" shortcut="ArrowRight">
          <button className={styles.navBtn} onClick={() => navigate(1)} disabled={!canNext}>
            <ToolbarChevronIcon direction="right" />
          </button>
        </KbdTooltip>
      </div>
    </div>,
    document.body,
  );
}
