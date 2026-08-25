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
