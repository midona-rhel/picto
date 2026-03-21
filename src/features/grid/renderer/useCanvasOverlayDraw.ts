import { useCallback, type RefObject } from 'react';
import { drawCanvasOverlayLayer } from './canvasGridDrawHelpers';
import type { MasonryImageItem } from '../shared';
import type { LayoutItem } from '../layoutMath';

interface ThemeState {
  primaryColor: string;
  textPrimary: string;
  textTertiary: string;
  placeholderBg: string;
  borderRadius: number;
  innerBorder: string;
}

interface LastVisibleState {
  startIdx: number;
  endIdx: number;
  visibleIndices: number[] | null;
  visibleIterEnd: number;
  scrollTop: number;
  cssH: number;
  th: number;
  br: number;
}

function ensureCanvasSize(canvas: HTMLCanvasElement, dpr: number): [number, number] {
  const cssW = canvas.clientWidth;
  const cssH = canvas.clientHeight;
  const bufW = Math.round(cssW * dpr);
  const bufH = Math.round(cssH * dpr);
  if (canvas.width !== bufW || canvas.height !== bufH) {
    canvas.width = bufW;
    canvas.height = bufH;
  }
  return [cssW, cssH];
}

export function useCanvasOverlayDraw(args: {
  lastVisibleRef: { current: LastVisibleState | null };
  overlayCanvasRef: { current: HTMLCanvasElement | null };
  overlayCtxRef: { current: CanvasRenderingContext2D | null };
  themeRef: { current: ThemeState | null };
  layoutRef: { current: { positions: LayoutItem[] } };
  imagesRef: { current: MasonryImageItem[] };
  selectedHashesRef: { current: Set<string> };
  hoveredTileRef: { current: number | null };
  marqueeRectRefProp?: RefObject<{ left: number; top: number; width: number; height: number } | null>;
  marqueeRectRef: { current: { left: number; top: number; width: number; height: number } | null };
  marqueeHitHashesRefProp?: RefObject<Set<string> | null>;
  marqueeHitHashesRef: { current: Set<string> | null };
  marqueeActiveRef: { current: boolean };
  isScrollingRef: { current: boolean };
  reorderDragRef: { current: { started?: boolean; dropIndex: number | null; dropSide: 'left' | 'right' | null } | null };
  gap: number;
  zoomBtnSize: number;
}) {
  const {
    lastVisibleRef,
    overlayCanvasRef,
    overlayCtxRef,
    themeRef,
    layoutRef,
    imagesRef,
    selectedHashesRef,
    hoveredTileRef,
    marqueeRectRefProp,
    marqueeRectRef,
    marqueeHitHashesRefProp,
    marqueeHitHashesRef,
    marqueeActiveRef,
    isScrollingRef,
    reorderDragRef,
    gap,
    zoomBtnSize,
  } = args;

  return useCallback(() => {
    const vis = lastVisibleRef.current;
    if (!vis) return;

    const overlay = overlayCanvasRef.current;
    if (!overlay) return;

    if (!overlayCtxRef.current || overlayCtxRef.current.canvas !== overlay) {
      overlayCtxRef.current = overlay.getContext('2d', { alpha: true, desynchronized: true });
    }
    const ctx = overlayCtxRef.current;
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const [cssW, cssHOverlay] = ensureCanvasSize(overlay, dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssHOverlay);

    const theme = themeRef.current;
    if (!theme) return;

    const positions = layoutRef.current.positions;
    const imgs = imagesRef.current;
    const selected = selectedHashesRef.current;
    const hoveredIdx = hoveredTileRef.current;
    const mRect = marqueeRectRefProp?.current ?? marqueeRectRef.current;
    const mHits = marqueeHitHashesRefProp?.current ?? marqueeHitHashesRef.current;
    const isScrolling = isScrollingRef.current;
    const { startIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br } = vis;

    drawCanvasOverlayLayer({
      ctx,
      positions,
      imgs,
      theme,
      visible: { startIdx, visibleIndices, visibleIterEnd, scrollTop, cssH, th, br },
      selected,
      hoveredIdx,
      marqueeRect: mRect,
      marqueeHitHashes: mHits,
      marqueeActive: marqueeActiveRef.current,
      isScrolling,
      zoomBtnSize,
      gap,
      reorderDrop: reorderDragRef.current?.started
        ? {
            dropIndex: reorderDragRef.current.dropIndex,
            dropSide: reorderDragRef.current.dropSide,
          }
        : null,
    });

  }, [
    gap,
    hoveredTileRef,
    imagesRef,
    isScrollingRef,
    lastVisibleRef,
    layoutRef,
    marqueeActiveRef,
    marqueeHitHashesRef,
    marqueeHitHashesRefProp,
    marqueeRectRef,
    marqueeRectRefProp,
    overlayCanvasRef,
    overlayCtxRef,
    reorderDragRef,
    selectedHashesRef,
    themeRef,
    zoomBtnSize,
  ]);
}
