import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import { createDragIcon } from '../../../shared/lib/createDragIcon';
import { imageDrag } from '../../../shared/lib/imageDrag';
import { mediaFileUrl, mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import { getCurrentWebview } from '#desktop/api';
import { computeCanvasReorderTarget } from './canvasHitTesting';
import { isVideoMime, type MasonryItem } from '../shared';
import type { GridViewMode } from '../runtime';
import type { LayoutResult } from '../layoutMath';
import type { VideoScrubRect } from '../VideoScrubOverlay';
import { getContainRect } from './canvasGridPrimitives';

interface HoverPreviewData {
  hash: string;
  mime: string;
}

const PRELOAD_DELAY_MS = 100;
const PREVIEW_DELAY_MS = 200;
const VIDEO_SCRUB_DELAY_MS = 500;
const DRAG_THRESHOLD_SQ = 25;
const EDGE_ZONE = 60;
const MAX_SCROLL_SPEED = 12;

export function useCanvasPointerInteractions(args: {
  hitTest: (clientX: number, clientY: number) => number | null;
  isZoomButtonHit: (clientX: number, clientY: number, tileIdx: number) => boolean;
  canvasRef: RefObject<HTMLCanvasElement | null>;
  scrollContainerRef?: RefObject<HTMLDivElement | null>;
  getScrollMetrics: () => { localScrollTop: number; canvasTopInScroll: number; viewportHeight: number };
  imagesRef: React.MutableRefObject<MasonryItem[]>;
  selectedHashesRef: React.MutableRefObject<Set<string>>;
  layoutRef: React.MutableRefObject<LayoutResult>;
  viewModeRef: React.MutableRefObject<GridViewMode>;
  viewportHeightRef: React.MutableRefObject<number>;
  reorderModeRef: React.MutableRefObject<boolean>;
  dragDisabledRef: React.MutableRefObject<boolean>;
  onReorderRef: React.MutableRefObject<((movedHashes: string[], targetIndex: number) => void) | undefined>;
  scrollTopRef: { current: number };
  textHeightRef: { current: number };
  hoveredTileRef: { current: number | null };
  marqueeActiveRef: { current: boolean };
  markDirty: (lanes: 'base' | 'overlay' | 'both') => void;
  videoScrubIdxRef?: { current: number | null };
  thumbnailFitModeRef?: { current: 'cover' | 'contain' };
}) {
  const {
    hitTest,
    isZoomButtonHit,
    canvasRef,
    scrollContainerRef,
    getScrollMetrics,
    imagesRef,
    selectedHashesRef,
    layoutRef,
    viewModeRef,
    viewportHeightRef,
    reorderModeRef,
    dragDisabledRef,
    onReorderRef,
    scrollTopRef,
    textHeightRef,
    hoveredTileRef,
    marqueeActiveRef,
    markDirty,
    videoScrubIdxRef: externalVideoScrubIdxRef,
    thumbnailFitModeRef,
  } = args;

  // ── Hover state ──────────────────────────────────────────────────────
  const [hoverPreview, setHoverPreview] = useState<HoverPreviewData | null>(null);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const preloadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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

  // ── Drag state ───────────────────────────────────────────────────────
  const dragJustEndedRef = useRef(false);
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
  const autoScrollRef = useRef<{ rafId: number | null; speed: number; armed: boolean }>({
    rafId: null,
    speed: 0,
    armed: false,
  });

  // ── Hover helpers ────────────────────────────────────────────────────
  const showHoverPreview = useCallback((image: MasonryItem | undefined) => {
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
        // Start preloading the full image at 100ms (before the portal shows)
        if (!preloadTimerRef.current) {
          preloadTimerRef.current = setTimeout(() => {
            preloadTimerRef.current = null;
            if (image) {
              const url = mediaFileUrl(image.thumbnail_hash || image.hash, image.mime);
              if (!hoverPreviewLoadedCache.has(url)) {
                const img = new Image();
                img.src = url;
              }
            }
          }, PRELOAD_DELAY_MS);
        }
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
      if (preloadTimerRef.current) {
        clearTimeout(preloadTimerRef.current);
        preloadTimerRef.current = null;
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

            // In contain mode, match the contained image area (same as canvas badge positioning)
            let rx = pos.x;
            let ry = pos.y - scrollTopRef.current;
            let rw = pos.w;
            let rh = imageH;
            if (thumbnailFitModeRef?.current === 'contain' && image.aspectRatio) {
              const cr = getContainRect(image.aspectRatio, pos.x, pos.y - scrollTopRef.current, pos.w, imageH);
              rx = cr.x;
              ry = cr.y;
              rw = cr.w;
              rh = cr.h;
            }

            const rect: VideoScrubRect = {
              left: Math.round(canvasRect.left + rx) - 1,
              top: Math.round(canvasRect.top + ry) - 1,
              width: Math.round(rw) + 1,
              height: Math.round(rh) + 1,
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

  // ── Drag helpers ─────────────────────────────────────────────────────
  const stopAutoScroll = useCallback(() => {
    const state = autoScrollRef.current;
    if (state.rafId != null) {
      cancelAnimationFrame(state.rafId);
      state.rafId = null;
    }
    state.speed = 0;
  }, []);

  const startAutoScroll = useCallback(() => {
    const state = autoScrollRef.current;
    if (state.rafId != null) return;
    const tick = () => {
      const scrollElement = scrollContainerRef?.current;
      if (!scrollElement || state.speed === 0) {
        state.rafId = null;
        return;
      }
      scrollElement.scrollTop += state.speed;
      state.rafId = requestAnimationFrame(tick);
    };
    state.rafId = requestAnimationFrame(tick);
  }, [scrollContainerRef]);

  const clearDragState = useCallback(() => {
    stopAutoScroll();
    autoScrollRef.current.armed = false;
    draggedHashSetRef.current = null;
    reorderDragRef.current = null;
    markDirty('overlay');
  }, [markDirty, stopAutoScroll]);

  const computeReorderTarget = useCallback((clientX: number, clientY: number, draggedSet: Set<string>) => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    const mouseX = clientX - rect.left;
    const scrollTop = getScrollMetrics().localScrollTop;
    const mouseY = clientY - rect.top + scrollTop;
    const positions = layoutRef.current.positions;
    const target = computeCanvasReorderTarget({
      positions,
      images: imagesRef.current,
      mouseX,
      mouseY,
      scrollTop,
      viewportHeight: viewportHeightRef.current,
      draggedSet,
    });
    if (target) return target;

    if (positions.length > 0) {
      const last = positions[positions.length - 1];
      if (mouseY >= last.y && mouseY <= last.y + last.h && mouseX > last.x + last.w) {
        return { index: positions.length - 1, side: 'right' as const };
      }
    }
    return null;
  }, [
    canvasRef,
    getScrollMetrics,
    imagesRef,
    layoutRef,
    viewModeRef,
    viewportHeightRef,
  ]);

  const handlePointerDown = useCallback((event: React.PointerEvent) => {
    if (event.button !== 0 || !event.isPrimary) return;
    const index = hitTest(event.clientX, event.clientY);
    if (index == null) return;
    const image = imagesRef.current[index];
    if (!image) return;

    event.stopPropagation();
    if (isZoomButtonHit(event.clientX, event.clientY, index)) return;
    if (dragDisabledRef.current) return;

    const state = { hash: image.hash, startX: event.clientX, startY: event.clientY, started: false };
    dragStateRef.current = state;

    const selected = imageDrag.getSelectedHashes();
    const hashes = selectedHashesRef.current.has(image.hash) && selected.size > 0 ? Array.from(selected) : [image.hash];

    const cleanup = () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
    };

    let nativeDragStarted = false;
    let sessionId: number | null = null;

    const handleMove = (moveEvent: PointerEvent) => {
      if (nativeDragStarted) return;

      const dx = moveEvent.clientX - state.startX;
      const dy = moveEvent.clientY - state.startY;
      if (!state.started && dx * dx + dy * dy <= DRAG_THRESHOLD_SQ) return;

      if (!state.started) {
        state.started = true;
        dragJustEndedRef.current = false;
        draggedHashSetRef.current = new Set(hashes);
        sessionId = imageDrag.startNativeDragSession(hashes);

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

        // Start custom pointer-based drag (DragGhost + elementFromPoint)
        // Use thumbnail_hash for drag ghost (collections have synthetic hashes with no thumbnail)
        const thumbUrls = hashes.slice(0, 3).map((h) => {
          const img = imagesRef.current.find((i) => i.hash === h);
          return mediaThumbnailUrl(img?.thumbnail_hash || h);
        });
        // Inside the app, count = number of entities being dragged (collection = 1 entity).
        // Native OS drag expands collections to member files with a different count.
        imageDrag.start(hashes, thumbUrls, moveEvent.clientX, moveEvent.clientY);
      }

      // Track pointer for sidebar folder drops + reorder
      imageDrag.move(moveEvent.clientX, moveEvent.clientY);

      // Reorder: update drop indicator via pointer position
      if (reorderModeRef.current && reorderDragRef.current) {
        const draggedSet = draggedHashSetRef.current;
        if (draggedSet) {
          const target = computeReorderTarget(moveEvent.clientX, moveEvent.clientY, draggedSet);
          const rstate = reorderDragRef.current;
          const nextIndex = target?.index ?? null;
          const nextSide = target?.side ?? null;
          if (nextIndex !== rstate.dropIndex || nextSide !== rstate.dropSide) {
            rstate.dropIndex = nextIndex;
            rstate.dropSide = nextSide;
            markDirty('overlay');
          }
        }

        // Auto-scroll at edges
        const scrollElement = scrollContainerRef?.current;
        if (scrollElement) {
          const rect = scrollElement.getBoundingClientRect();
          const distFromTop = moveEvent.clientY - rect.top;
          const distFromBottom = rect.bottom - moveEvent.clientY;
          const autoScroll = autoScrollRef.current;
          if (distFromTop > EDGE_ZONE && distFromBottom > EDGE_ZONE) {
            autoScroll.armed = true;
          }
          if (autoScroll.armed && distFromTop < EDGE_ZONE) {
            autoScroll.speed = -Math.round(MAX_SCROLL_SPEED * (1 - distFromTop / EDGE_ZONE));
            startAutoScroll();
          } else if (autoScroll.armed && distFromBottom < EDGE_ZONE) {
            autoScroll.speed = Math.round(MAX_SCROLL_SPEED * (1 - distFromBottom / EDGE_ZONE));
            startAutoScroll();
          } else {
            stopAutoScroll();
          }
        }
      }

      // Check if pointer left the window — escalate to native OS drag
      const { clientX, clientY } = moveEvent;
      if (clientX <= 0 || clientY <= 0 || clientX >= window.innerWidth || clientY >= window.innerHeight) {
        nativeDragStarted = true;
        dragJustEndedRef.current = true;
        imageDrag.forceEnd();
        stopAutoScroll();
        cleanup();

        const thumbnailUrl = mediaThumbnailUrl(image.thumbnail_hash || image.hash);
        const icon = new Image();
        const doStartDrag = (iconDataUrl?: string | null) => {
          getCurrentWebview()
            .startNativeDrag(hashes, iconDataUrl)
            .catch(() => {
              if (sessionId != null) imageDrag.clearNativeDragSession(sessionId);
            });
        };
        // For native OS drag, expand collection counts to total files
        const nativeFileCount = hashes.reduce((sum, h) => {
          const img = imagesRef.current.find((i) => i.hash === h);
          return sum + (img?.is_collection && img.collection_item_count ? img.collection_item_count : 1);
        }, 0);
        icon.onload = () => doStartDrag(createDragIcon(icon, nativeFileCount));
        icon.onerror = () => doStartDrag();
        icon.src = thumbnailUrl;
      }
    };

    const handleUp = () => {
      cleanup();
      stopAutoScroll();

      if (state.started && !nativeDragStarted) {
        // Internal drag ended — suppress the click that follows pointerup
        dragJustEndedRef.current = true;
        // Internal drag ended — execute action
        if (reorderModeRef.current && reorderDragRef.current) {
          const rstate = reorderDragRef.current;
          if (rstate.dropIndex != null && rstate.dropSide != null) {
            const targetIndex = rstate.dropSide === 'right' ? rstate.dropIndex + 1 : rstate.dropIndex;
            onReorderRef.current?.(rstate.draggedHashes, targetIndex);
          }
        }

        imageDrag.end();
        if (sessionId != null) imageDrag.clearNativeDragSession(sessionId);
        clearDragState();
      }
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  }, [
    clearDragState,
    computeReorderTarget,
    dragDisabledRef,
    hitTest,
    imagesRef,
    isZoomButtonHit,
    markDirty,
    reorderModeRef,
    scrollContainerRef,
    selectedHashesRef,
    startAutoScroll,
    stopAutoScroll,
  ]);

  const handleCanvasDragOver = useCallback((event: React.DragEvent) => {
    const draggedSet = draggedHashSetRef.current;
    if (!draggedSet) return;

    event.preventDefault();
    event.stopPropagation();

    if (!reorderModeRef.current) return;

    event.dataTransfer.dropEffect = 'move';
    const target = computeReorderTarget(event.clientX, event.clientY, draggedSet);
    const state = reorderDragRef.current;
    if (!state) return;

    const nextIndex = target?.index ?? null;
    const nextSide = target?.side ?? null;
    if (nextIndex !== state.dropIndex || nextSide !== state.dropSide) {
      state.dropIndex = nextIndex;
      state.dropSide = nextSide;
      markDirty('overlay');
    }

    const scrollElement = scrollContainerRef?.current;
    if (!scrollElement) return;

    const rect = scrollElement.getBoundingClientRect();
    const distFromTop = event.clientY - rect.top;
    const distFromBottom = rect.bottom - event.clientY;
    const autoScroll = autoScrollRef.current;
    if (distFromTop > EDGE_ZONE && distFromBottom > EDGE_ZONE) {
      autoScroll.armed = true;
    }

    if (autoScroll.armed && distFromTop < EDGE_ZONE) {
      autoScroll.speed = -Math.round(MAX_SCROLL_SPEED * (1 - distFromTop / EDGE_ZONE));
      startAutoScroll();
    } else if (autoScroll.armed && distFromBottom < EDGE_ZONE) {
      autoScroll.speed = Math.round(MAX_SCROLL_SPEED * (1 - distFromBottom / EDGE_ZONE));
      startAutoScroll();
    } else {
      stopAutoScroll();
    }
  }, [computeReorderTarget, markDirty, reorderModeRef, scrollContainerRef, startAutoScroll, stopAutoScroll]);

  const handleCanvasDrop = useCallback((event: React.DragEvent) => {
    const draggedSet = draggedHashSetRef.current;
    if (!draggedSet) return;

    event.preventDefault();
    event.stopPropagation();
    stopAutoScroll();
    autoScrollRef.current.armed = false;

    if (reorderModeRef.current) {
      const state = reorderDragRef.current;
      const target = state?.dropIndex != null && state.dropSide != null
        ? { index: state.dropIndex, side: state.dropSide }
        : computeReorderTarget(event.clientX, event.clientY, draggedSet);
      if (target && state) {
        const targetIndex = target.side === 'right' ? target.index + 1 : target.index;
        onReorderRef.current?.(state.draggedHashes, targetIndex);
      }
    }

    imageDrag.clearNativeDragSession();
    clearDragState();
  }, [clearDragState, computeReorderTarget, onReorderRef, reorderModeRef, stopAutoScroll]);

  const handleCanvasDragLeave = useCallback(() => {
    stopAutoScroll();
  }, [stopAutoScroll]);

  // ── Cleanup ──────────────────────────────────────────────────────────
  useEffect(() => {
    return () => {
      if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
      if (hoverHideTimerRef.current) clearTimeout(hoverHideTimerRef.current);
      if (preloadTimerRef.current) clearTimeout(preloadTimerRef.current);
      if (videoScrubTimerRef.current) clearTimeout(videoScrubTimerRef.current);
    };
  }, []);

  return {
    // Hover returns
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
      if (preloadTimerRef.current) {
        clearTimeout(preloadTimerRef.current);
        preloadTimerRef.current = null;
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
    // Drag returns
    dragJustEndedRef,
    reorderDragRef,
    handlePointerDown,
    handleCanvasDragOver,
    handleCanvasDrop,
    handleCanvasDragLeave,
    clearDragState,
  };
}

// ── Hover preview loaded cache (re-exported for HoverPreviewPortal) ────
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
