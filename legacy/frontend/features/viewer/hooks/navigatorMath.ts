export interface NavigatorMathZoomState {
  scale: number;
  tx: number;
  ty: number;
}

export interface NavigatorMathImageSize {
  width: number;
  height: number;
}

export interface NavigatorMathRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function computeNavigatorRect(
  zoomState: NavigatorMathZoomState,
  imageSize: NavigatorMathImageSize,
  containerSize: { w: number; h: number },
): NavigatorMathRect | null {
  const cw = containerSize.w;
  const ch = containerSize.h;
  const scaledWidth = imageSize.width * zoomState.scale;
  const scaledHeight = imageSize.height * zoomState.scale;
  if (scaledWidth < cw + 1 && scaledHeight < ch + 1) return null;

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
