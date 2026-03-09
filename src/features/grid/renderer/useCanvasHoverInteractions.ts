import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import { isVideoMime, type MasonryImageItem } from '../shared';
import type { LayoutResult } from '../layoutMath';
import type { VideoScrubRect } from '../VideoScrubOverlay';

interface HoverPreviewData {
  hash: string;
  mime: string;
}

const PREVIEW_DELAY_MS = 200;
const VIDEO_SCRUB_DELAY_MS = 500;

export function useCanvasHoverInteractions(args: {
  hitTest: (clientX: number, clientY: number) => number | null;
  isZoomButtonHit: (clientX: number, clientY: number, tileIdx: number) => boolean;
  imagesRef: { current: MasonryImageItem[] };
  layoutRef: { current: LayoutResult };
  canvasRef: RefObject<HTMLCanvasElement | null>;
  scrollTopRef: { current: number };
  textHeightRef: { current: number };
  hoveredTileRef: { current: number | null };
  marqueeActiveRef: { current: boolean };
  markDirty: (lanes: 'base' | 'overlay' | 'both') => void;
  videoScrubIdxRef?: { current: number | null };
}) {
  const {
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
    videoScrubIdxRef: externalVideoScrubIdxRef,
  } = args;

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
  const internalVideoScrubIdxRef = useRef<number | null>(null);
  const videoScrubIdxRef = externalVideoScrubIdxRef ?? internalVideoScrubIdxRef;

  const showHoverPreview = useCallback((image: MasonryImageItem | undefined) => {
    if (!image || isVideoMime(image.mime) || image.is_collection) return;
    if (hoverHideTimerRef.current) {
      clearTimeout(hoverHideTimerRef.current);
      hoverHideTimerRef.current = null;
    }
    setHoverPreview((prev) => (
      prev && prev.hash === image.hash && prev.mime === image.mime
        ? prev
        : { hash: image.hash, mime: image.mime }
    ));
  }, []);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (marqueeActiveRef.current) return;

    const idx = hitTest(e.clientX, e.clientY);
    const prevIdx = hoveredTileRef.current;

    if (idx !== prevIdx) {
      hoveredTileRef.current = idx;
      markDirty('overlay');
    }

    if (idx != null && isZoomButtonHit(e.clientX, e.clientY, idx)) {
      const image = imagesRef.current[idx];
      const isPreviewable = image && !isVideoMime(image.mime) && !image.is_collection;
      if (hoverHideTimerRef.current) {
        clearTimeout(hoverHideTimerRef.current);
        hoverHideTimerRef.current = null;
      }
      if (isPreviewable && !hoverTimerRef.current) {
        hoverTimerRef.current = setTimeout(() => {
          showHoverPreview(image);
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

    if (idx !== videoScrubIdxRef.current) {
      if (videoScrubTimerRef.current) {
        clearTimeout(videoScrubTimerRef.current);
        videoScrubTimerRef.current = null;
      }
      videoScrubIdxRef.current = idx;
      setVideoScrub(null);

      if (idx != null) {
        const image = imagesRef.current[idx];
        const durationMs = image?.duration_ms;
        if (image && isVideoMime(image.mime) && durationMs != null && durationMs > 0) {
          videoScrubTimerRef.current = setTimeout(() => {
            videoScrubTimerRef.current = null;
            const canvas = canvasRef.current;
            if (!canvas) return;
            const canvasRect = canvas.getBoundingClientRect();
            const pos = layoutRef.current.positions[idx];
            if (!pos) return;
            const imageH = pos.h - textHeightRef.current;
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
              durationSec: durationMs / 1000,
              rect,
            });
          }, VIDEO_SCRUB_DELAY_MS);
        }
      }
    }
  }, [
    canvasRef,
    hitTest,
    hoveredTileRef,
    imagesRef,
    isZoomButtonHit,
    layoutRef,
    marqueeActiveRef,
    markDirty,
    scrollTopRef,
    showHoverPreview,
    textHeightRef,
  ]);

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
    if (videoScrubTimerRef.current) {
      clearTimeout(videoScrubTimerRef.current);
      videoScrubTimerRef.current = null;
    }
  }, [hoveredTileRef, markDirty]);

  useEffect(() => {
    return () => {
      if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
      if (hoverHideTimerRef.current) clearTimeout(hoverHideTimerRef.current);
      if (videoScrubTimerRef.current) clearTimeout(videoScrubTimerRef.current);
    };
  }, []);

  return {
    hoverPreview,
    setHoverPreview,
    showHoverPreview,
    handleMouseMove,
    handleMouseLeave,
    videoScrub,
    setVideoScrub,
    videoScrubIdxRef,
    clearPendingHoverTimers() {
      if (hoverTimerRef.current) {
        clearTimeout(hoverTimerRef.current);
        hoverTimerRef.current = null;
      }
      if (hoverHideTimerRef.current) {
        clearTimeout(hoverHideTimerRef.current);
        hoverHideTimerRef.current = null;
      }
    },
    clearPendingVideoScrubTimer() {
      if (videoScrubTimerRef.current) {
        clearTimeout(videoScrubTimerRef.current);
        videoScrubTimerRef.current = null;
      }
    },
    clearVideoScrubIndex() {
      videoScrubIdxRef.current = null;
    },
  };
}

const hoverPreviewLoadedCache = new Set<string>();

export function useHoverPreviewLoaded(fullUrl: string) {
  const [loaded, setLoaded] = useState(() => hoverPreviewLoadedCache.has(fullUrl));

  useEffect(() => {
    setLoaded(hoverPreviewLoadedCache.has(fullUrl));
  }, [fullUrl]);

  const markLoaded = useCallback(() => {
    hoverPreviewLoadedCache.add(fullUrl);
    setLoaded(true);
  }, [fullUrl]);

  return { loaded, markLoaded };
}

export type { HoverPreviewData };
