/**
 * Navigator/minimap math — computes the viewport rect in normalized 0–1 coordinates.
 * Returns null when the image fits entirely within the container (no minimap needed).
 */

export interface NavigatorRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

const FIT_ROUNDING_TOLERANCE_PX = 3;

export function computeNavigatorRect(
  zoomState: { scale: number; tx: number; ty: number },
  imageSize: { width: number; height: number },
  containerSize: { w: number; h: number },
): NavigatorRect | null {
  const cw = containerSize.w;
  const ch = containerSize.h;
  const scaledWidth = imageSize.width * zoomState.scale;
  const scaledHeight = imageSize.height * zoomState.scale;

  // Native aspect-ratio resizing can leave the fitted image a few fractional
  // pixels larger than the content area. Treat that as fitted so the navigator
  // cannot flicker at the boundary while the window is being resized.
  if (
    scaledWidth <= cw + FIT_ROUNDING_TOLERANCE_PX
    && scaledHeight <= ch + FIT_ROUNDING_TOLERANCE_PX
  ) return null;

  const imageLeft = cw / 2 + zoomState.tx - scaledWidth / 2;
  const imageTop = ch / 2 + zoomState.ty - scaledHeight / 2;
  const visibleLeftPx = Math.max(0, -imageLeft);
  const visibleTopPx = Math.max(0, -imageTop);
  const visibleRightPx = Math.min(scaledWidth, cw - imageLeft);
  const visibleBottomPx = Math.min(scaledHeight, ch - imageTop);

  const visibleWidthPx = Math.max(0, visibleRightPx - visibleLeftPx);
  const visibleHeightPx = Math.max(0, visibleBottomPx - visibleTopPx);

  return {
    x: Math.max(0, Math.min(1, (visibleLeftPx / zoomState.scale) / imageSize.width)),
    y: Math.max(0, Math.min(1, (visibleTopPx / zoomState.scale) / imageSize.height)),
    w: Math.max(0, Math.min(1, (visibleWidthPx / zoomState.scale) / imageSize.width)),
    h: Math.max(0, Math.min(1, (visibleHeightPx / zoomState.scale) / imageSize.height)),
  };
}
