/**
 * Viewport metrics — reads fresh from DOM every frame.
 *
 * No caching (unlike legacy 2ms cache that caused jitter).
 * Returns a snapshot of scroll/viewport state for frame-coherent drawing.
 */

export interface ViewportSnapshot {
  scrollTop: number;
  viewportHeight: number;
  containerWidth: number;
  dpr: number;
}

export function snapshotViewport(container: HTMLDivElement): ViewportSnapshot {
  return {
    scrollTop: container.scrollTop,
    viewportHeight: container.clientHeight,
    containerWidth: container.clientWidth,
    dpr: window.devicePixelRatio || 1,
  };
}

/**
 * Ensure canvas physical size matches CSS size × DPR.
 * Returns true if the canvas was resized (needs full redraw).
 */
export function ensureCanvasSize(
  canvas: HTMLCanvasElement,
  cssWidth: number,
  cssHeight: number,
  dpr: number,
): boolean {
  const physW = Math.round(cssWidth * dpr);
  const physH = Math.round(cssHeight * dpr);

  if (canvas.width === physW && canvas.height === physH) return false;

  canvas.width = physW;
  canvas.height = physH;
  canvas.style.width = `${cssWidth}px`;
  canvas.style.height = `${cssHeight}px`;
  return true;
}
