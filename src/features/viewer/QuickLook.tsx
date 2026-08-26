/**
 * QuickLook — peek overlay (Space key).
 *
 * Standalone image viewer with minimal chrome. Same zoom/image model as MediaView
 * (frame at left:50% top:50%, zoom hook applies translate(calc(-50% + tx)) scale(s)).
 * No navigator minimap. Fade-in on open. Exit button top-right, nav toolbar bottom.
 */

import { useState, useEffect, useLayoutEffect, useMemo, useRef, useCallback } from 'react';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import { mediaThumbnailUrl, mediaFileUrl } from '../../shared/lib/mediaUrl';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import * as entityMutations from '../../controllers/entityMutations';
import { filesController } from '../../controllers/filesController';
import { useImageZoom, type ImageSize } from './hooks/useImageZoom';
import { useMediaImagePipeline } from '../../shared/hooks/useMediaImagePipeline';
import { useRecordMediaView } from './hooks/useRecordMediaView';
import { VideoPlayer } from './video/VideoPlayer';
import { DetailMediaRenderer } from './document/DetailMediaRenderer';
import { detailRendererKind } from './document/detailRendererKind';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { ImageCrossfadeFrame } from './ImageCrossfadeFrame';
import { QuickLookHost } from './QuickLookHost';
import styles from './QuickLook.module.css';
import { useViewerEntityContextMenu } from './useViewerEntityContextMenu';
import { usePreviewPreferences } from './usePreviewPreferences';

export interface QuickLookProps {
  items: CanonicalEntityGridItem[];
  currentIndex: number;
  totalCount?: number | null;
  onNavigate: (delta: number) => void;
  onClose: (exitItemId: number) => void;
  onLoadMore?: () => void;
}

interface QuickLookContentProps extends QuickLookProps {
  thumbnailReady?: boolean;
  onReady: () => void;
}

export function QuickLookContent({
  items, currentIndex, onNavigate, onClose, onLoadMore, onReady, thumbnailReady = false,
}: QuickLookContentProps) {
  const currentItem = items[currentIndex] ?? null;
  const currentItemId = currentItem?.item_id ?? 0;
  const currentHash = currentItem?.display_file_hash ?? '';
  useRecordMediaView(currentItemId);
  const currentMime = currentItem?.display_mime_type ?? '';
  const previewPreferences = usePreviewPreferences();
  const rendererKind = detailRendererKind(currentMime);
  const isImage = rendererKind === 'image';
  const isVideo = rendererKind === 'video';
  const isAudio = rendererKind === 'audio';
  const thumbHash = currentHash;
  const contextMenu = useViewerEntityContextMenu({
    hash: currentHash || null,
    itemId: currentItem?.item_id,
    kind: currentItem?.kind,
    lifecycle: currentItem?.lifecycle,
    name: currentItem?.name,
    mime: currentMime,
    width: currentItem?.pixel_width,
    height: currentItem?.pixel_height,
  });

  // Refs
  const containerRef = useRef<HTMLDivElement>(null);
  const imageFrameRef = useRef<HTMLDivElement>(null);
  const fullImgRef = useRef<HTMLImageElement>(null);

  // Media pipeline (before imageSize — derives from displayedHash)
  const neighborHashes = useMemo(() => {
    const r: string[] = [];
    const prev = items[currentIndex - 1];
    const next = items[currentIndex + 1];
    if (prev) r.push(prev.display_file_hash);
    if (next) r.push(next.display_file_hash);
    return r;
  }, [items, currentIndex]);

  const pipeline = useMediaImagePipeline({
    hash: currentHash || null,
    thumbnailHash: currentItem?.display_file_hash ?? null,
    mime: currentMime,
    isVideo: !isImage,
    neighborHashes,
  });

  useEffect(() => {
    if (!isImage || thumbnailReady || pipeline.thumbLoaded || pipeline.fullVisible) onReady();
  }, [isImage, onReady, pipeline.fullVisible, pipeline.thumbLoaded, thumbnailReady]);

  // Image size from displayed item
  const displayedItem = pipeline.displayedHash
    ? items.find((it) => it.display_file_hash === pipeline.displayedHash) ?? currentItem
    : currentItem;
  const imageSize = useMemo<ImageSize | null>(() => {
    if (!displayedItem?.pixel_width || !displayedItem?.pixel_height) return null;
    return { width: displayedItem.pixel_width, height: displayedItem.pixel_height };
  }, [pipeline.displayedHash]); // eslint-disable-line react-hooks/exhaustive-deps

  // Zoom/pan
  const zoom = useImageZoom(containerRef, imageSize, [imageFrameRef], {
    macTrackpadGestures: previewPreferences.viewerTrackpadGestures,
  });

  // Fit when displayed image changes
  useLayoutEffect(() => {
    if (!imageSize) return;
    const el = containerRef.current;
    if (!el) return;
    const cw = el.clientWidth;
    const ch = el.clientHeight;
    if (cw === 0 || ch === 0) return;
    const scale = previewPreferences.imageDefaultZoom === 'actual'
      ? 1
      : Math.min(cw / imageSize.width, ch / imageSize.height);
    zoom.setState({ scale, tx: 0, ty: 0 });
  }, [pipeline.displayedHash, imageSize, previewPreferences.imageDefaultZoom]); // eslint-disable-line react-hooks/exhaustive-deps

  const navigate = useCallback((delta: number) => {
    const nextIdx = currentIndex + delta;
    if (nextIdx < 0 || nextIdx >= items.length) {
      if (nextIdx >= items.length && onLoadMore) onLoadMore();
      return;
    }
    onNavigate(delta);
  }, [currentIndex, items.length, onNavigate, onLoadMore]);

  // Keyboard — registry defs for EU alternative keys
  useShortcutScope((e) => {
    const closeDef = getShortcut('view.closeDetail')!;
    const detailDef = getShortcut('view.detailView')!;
    const quicklookDef = getShortcut('view.quicklook')!;
    const prevDef = getShortcut('view.prevImage')!;
    const nextDef = getShortcut('view.nextImage')!;
    const fitDef = getShortcut('view.fitWindow')!;
    const zoomInDef = getShortcut('view.zoomIn')!;
    const zoomOutDef = getShortcut('view.zoomOut')!;
    const actualDef = getShortcut('view.actualSize')!;
    const copyDef = getShortcut('edit.copy')!;

      // Escape, Space (quicklook toggle), or Enter (detail toggle) all close
      if (matchesShortcutDef(e, closeDef) || matchesShortcutDef(e, quicklookDef) || matchesShortcutDef(e, detailDef)) { e.preventDefault(); onClose(currentItemId); return; }
      if (matchesShortcutDef(e, prevDef)) { e.preventDefault(); navigate(-1); return; }
      if (matchesShortcutDef(e, nextDef)) { e.preventDefault(); navigate(1); return; }
      if (matchesShortcutDef(e, copyDef)) {
        e.preventDefault();
        void filesController.copyTarget({ kind: 'explicit', item_ids: [currentItemId] });
        return;
      }
      if (isImage && matchesShortcutDef(e, fitDef)) { e.preventDefault(); zoom.fitToWindow(); return; }
      if (isImage && matchesShortcutDef(e, zoomInDef)) { e.preventDefault(); zoom.animateZoomTo(zoom.state.scale * 1.25); return; }
      if (isImage && matchesShortcutDef(e, zoomOutDef)) { e.preventDefault(); zoom.animateZoomTo(zoom.state.scale / 1.25); return; }
      if (isImage && matchesShortcutDef(e, actualDef)) { e.preventDefault(); zoom.fitActual(); return; }

      for (let rating = 0; rating <= 5; rating += 1) {
        if (!matchesShortcutDef(e, getShortcut(`rate.${rating}`)!)) continue;
        e.preventDefault();
        void entityMutations.setItemRating(currentItemId, rating);
        return;
      }
  }, { priority: 60 });

  if (!currentItem) return null;

  const thumbUrl = mediaThumbnailUrl(thumbHash);
  return (
    <div
      ref={containerRef}
      className={`${styles.imageArea} ${zoom.isDragging ? styles.dragging : ''}`}
      onMouseDown={isImage ? zoom.handlers.onMouseDown : undefined}
      onContextMenu={contextMenu.open}
    >
        {isAudio ? (
          <VideoPlayer
            key={currentHash}
            kind="audio"
            src={mediaFileUrl(thumbHash, currentMime)}
            waveformSrc={thumbUrl}
            muted={false}
          />
        ) : isVideo ? (
          <VideoPlayer
            key={currentHash}
            src={mediaFileUrl(thumbHash, currentMime)}
            autoPlay={previewPreferences.videoAutoPlay}
            loop={previewPreferences.videoLoop}
          />
        ) : !isImage ? (
          <DetailMediaRenderer
            hash={currentHash}
            mimeType={currentMime}
            displayName={currentItem.name}
          />
        ) : (
          <ImageCrossfadeFrame
            frameRef={imageFrameRef}
            fullImageRef={fullImgRef}
            imageSize={imageSize}
            thumbnailUrl={pipeline.thumbUrl || thumbUrl}
            fullUrl={pipeline.fullUrl}
            thumbnailVisible={thumbnailReady || pipeline.thumbLoaded}
            fullVisible={pipeline.fullVisible}
            imageRendering={previewPreferences.imageRendering}
            showTransparencyGrid={previewPreferences.showTransparencyGrid}
            onThumbnailLoad={pipeline.handleThumbLoad}
            onFullLoad={pipeline.handleFullLoad}
          />
        )}
      {contextMenu.menu}
    </div>
  );
}

export function QuickLook(props: QuickLookProps) {
  const [contentReady, setContentReady] = useState(false);
  const currentItemId = props.items[props.currentIndex]?.item_id ?? 0;
  const markReady = useCallback(() => setContentReady(true), []);

  return (
    <QuickLookHost
      contentReady={contentReady}
      currentIndex={props.currentIndex}
      totalCount={props.totalCount ?? props.items.length}
      canPrevious={props.currentIndex > 0}
      canNext={props.currentIndex < props.items.length - 1}
      onNavigate={props.onNavigate}
      onClose={() => props.onClose(currentItemId)}
    >
      <QuickLookContent {...props} onReady={markReady} />
    </QuickLookHost>
  );
}
