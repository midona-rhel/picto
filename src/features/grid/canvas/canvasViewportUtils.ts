export interface ViewportSnapshot {
  scrollTop: number;
  viewportHeight: number;
  containerWidth: number;
  dpr: number;
}

export interface CommittedViewportDimensions {
  width: number;
  height: number;
  dpr: number;
}

export const CANVAS_SCROLL_BUFFER_MARGIN_PX = 500;
export const CANVAS_SCROLL_RECENTER_DISTANCE_PX = 350;

/** Keep retained canvas pixels aligned with their content while the sticky viewport scrolls. */
export function canvasScrollBufferTransform(
  renderedScrollTop: number,
  visibleScrollTop: number,
  margin = CANVAS_SCROLL_BUFFER_MARGIN_PX,
): number {
  return renderedScrollTop - visibleScrollTop - margin;
}

export function canvasScrollBufferNeedsRecenter(
  renderedScrollTop: number,
  renderedViewportHeight: number,
  visibleScrollTop: number,
  viewportHeight: number,
  distance = CANVAS_SCROLL_RECENTER_DISTANCE_PX,
): boolean {
  if (renderedViewportHeight <= 0 || viewportHeight <= 0) return true;
  return visibleScrollTop < renderedScrollTop - distance
    || visibleScrollTop + viewportHeight > renderedScrollTop + renderedViewportHeight + distance;
}

export function canvasScrollBufferIsExhausted(
  renderedScrollTop: number,
  renderedViewportHeight: number,
  visibleScrollTop: number,
  viewportHeight: number,
  margin = CANVAS_SCROLL_BUFFER_MARGIN_PX,
): boolean {
  if (renderedViewportHeight <= 0 || viewportHeight <= 0) return true;
  return visibleScrollTop < renderedScrollTop - margin
    || visibleScrollTop + viewportHeight > renderedScrollTop + renderedViewportHeight + margin;
}

export function snapshotViewport(
  container: HTMLDivElement,
  committed?: CommittedViewportDimensions,
): ViewportSnapshot {
  return {
    scrollTop: container.scrollTop,
    viewportHeight: committed?.height ?? container.clientHeight,
    containerWidth: committed?.width ?? container.clientWidth,
    dpr: committed?.dpr ?? (window.devicePixelRatio || 1),
  };
}

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
