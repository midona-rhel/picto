import { useCallback, useRef } from 'react';
import { createDragIcon } from '../../../shared/lib/createDragIcon';
import { imageDrag } from '../../../shared/lib/imageDrag';
import { mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import { getCurrentWebview } from '#desktop/api';
import { computeCanvasReorderTarget } from './canvasHitTesting';
import type { GridViewMode } from '../runtime';
import type { MasonryImageItem } from '../shared';
import type { LayoutResult } from '../layoutMath';

const DRAG_THRESHOLD_SQ = 25;
const EDGE_ZONE = 60;
const MAX_SCROLL_SPEED = 12;

export function useCanvasDragInteractions(args: {
  hitTest: (clientX: number, clientY: number) => number | null;
  isZoomButtonHit: (clientX: number, clientY: number, tileIdx: number) => boolean;
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  scrollContainerRef?: React.RefObject<HTMLDivElement | null>;
  getScrollMetrics: () => { localScrollTop: number; canvasTopInScroll: number; viewportHeight: number };
  imagesRef: React.MutableRefObject<MasonryImageItem[]>;
  selectedHashesRef: React.MutableRefObject<Set<string>>;
  layoutRef: React.MutableRefObject<LayoutResult>;
  viewModeRef: React.MutableRefObject<GridViewMode>;
  viewportHeightRef: React.MutableRefObject<number>;
  bucketIndexRef: React.MutableRefObject<Map<number, number[]> | null>;
  waterfallSeenStateRef: React.MutableRefObject<{ seen: Uint32Array; token: number }>;
  waterfallHitIndicesRef: React.MutableRefObject<number[]>;
  reorderModeRef: React.MutableRefObject<boolean>;
  dragDisabledRef: React.MutableRefObject<boolean>;
  onReorderRef: React.MutableRefObject<((movedHashes: string[], targetIndex: number) => void) | undefined>;
  markDirty: (lanes: 'base' | 'overlay' | 'both') => void;
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
    bucketIndexRef,
    waterfallSeenStateRef,
    waterfallHitIndicesRef,
    reorderModeRef,
    dragDisabledRef,
    onReorderRef,
    markDirty,
  } = args;

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
      mode: viewModeRef.current,
      mouseX,
      mouseY,
      scrollTop,
      viewportHeight: viewportHeightRef.current,
      bucketIndex: bucketIndexRef.current,
      waterfallSeenState: waterfallSeenStateRef.current,
      waterfallHitIndices: waterfallHitIndicesRef.current,
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
    bucketIndexRef,
    canvasRef,
    getScrollMetrics,
    imagesRef,
    layoutRef,
    viewModeRef,
    viewportHeightRef,
    waterfallHitIndicesRef,
    waterfallSeenStateRef,
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

    const handleMove = (moveEvent: PointerEvent) => {
      const dx = moveEvent.clientX - state.startX;
      const dy = moveEvent.clientY - state.startY;
      if (state.started || dx * dx + dy * dy <= DRAG_THRESHOLD_SQ) return;
      state.started = true;
      cleanup();

      draggedHashSetRef.current = new Set(hashes);
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

      const sessionId = imageDrag.startNativeDragSession(hashes);
      const thumbnailUrl = mediaThumbnailUrl(image.hash);
      const icon = new Image();
      const startDrag = (iconDataUrl?: string | null) => {
        getCurrentWebview()
          .startNativeDrag(hashes, iconDataUrl)
          .catch(() => {
            imageDrag.clearNativeDragSession(sessionId);
          });
      };
      icon.onload = () => startDrag(createDragIcon(icon, hashes.length));
      icon.onerror = () => startDrag();
      icon.src = thumbnailUrl;
    };

    const handleUp = () => {
      cleanup();
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
  }, [
    dragDisabledRef,
    hitTest,
    imagesRef,
    isZoomButtonHit,
    reorderModeRef,
    selectedHashesRef,
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
      const target = computeReorderTarget(event.clientX, event.clientY, draggedSet);
      const state = reorderDragRef.current;
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
    const state = reorderDragRef.current;
    if (!state) return;
    state.dropIndex = null;
    state.dropSide = null;
    markDirty('overlay');
  }, [markDirty, stopAutoScroll]);

  return {
    reorderDragRef,
    handlePointerDown,
    handleCanvasDragOver,
    handleCanvasDrop,
    handleCanvasDragLeave,
    clearDragState,
  };
}
