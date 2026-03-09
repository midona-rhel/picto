import { useCallback } from 'react';
import { hitTestCanvasTile } from './canvasHitTesting';
import type { GridViewMode } from '../runtime';
import type { LayoutItem } from '../layoutMath';
import type { WaterfallSeenState } from '../layout/canvasVisibilityPlan';

export function useCanvasHitTesting(args: {
  canvasRef: { current: HTMLCanvasElement | null };
  layoutRef: { current: { positions: LayoutItem[] } };
  viewModeRef: { current: GridViewMode };
  scrollTopRef: { current: number };
  viewportHeightRef: { current: number };
  bucketIndexRef: { current: Map<number, number[]> | null };
  waterfallSeenStateRef: { current: WaterfallSeenState };
  waterfallHitIndicesRef: { current: number[] };
  textHeightRef: { current: number };
  zoomBtnSize: number;
}) {
  const {
    canvasRef,
    layoutRef,
    viewModeRef,
    scrollTopRef,
    viewportHeightRef,
    bucketIndexRef,
    waterfallSeenStateRef,
    waterfallHitIndicesRef,
    textHeightRef,
    zoomBtnSize,
  } = args;

  const hitTest = useCallback((clientX: number, clientY: number): number | null => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    return hitTestCanvasTile({
      positions: layoutRef.current.positions,
      mode: viewModeRef.current,
      mouseX: clientX - rect.left,
      mouseY: clientY - rect.top + scrollTopRef.current,
      scrollTop: scrollTopRef.current,
      viewportHeight: viewportHeightRef.current,
      bucketIndex: bucketIndexRef.current,
      waterfallSeenState: waterfallSeenStateRef.current,
      waterfallHitIndices: waterfallHitIndicesRef.current,
    });
  }, [
    bucketIndexRef,
    canvasRef,
    layoutRef,
    scrollTopRef,
    viewModeRef,
    viewportHeightRef,
    waterfallHitIndicesRef,
    waterfallSeenStateRef,
  ]);

  const isZoomButtonHit = useCallback((clientX: number, clientY: number, tileIdx: number): boolean => {
    const canvas = canvasRef.current;
    if (!canvas) return false;
    const rect = canvas.getBoundingClientRect();
    const mx = clientX - rect.left;
    const my = clientY - rect.top + scrollTopRef.current;
    const pos = layoutRef.current.positions[tileIdx];
    if (!pos) return false;
    const imageHeight = pos.h - textHeightRef.current;
    const bgW = zoomBtnSize + 4;
    const bgH = zoomBtnSize + 2;
    const zx = pos.x + pos.w - bgW;
    const zy = pos.y + imageHeight - bgH;
    return mx >= zx && mx < zx + bgW && my >= zy && my < zy + bgH;
  }, [canvasRef, layoutRef, scrollTopRef, textHeightRef, zoomBtnSize]);

  return {
    hitTest,
    isZoomButtonHit,
  };
}
