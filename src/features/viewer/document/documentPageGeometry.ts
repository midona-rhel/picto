export interface DocumentPageSize {
  width: number;
  height: number;
}

export interface DocumentCanvasGeometry {
  css: DocumentPageSize;
  pixels: DocumentPageSize;
  renderScale: number;
}

/** Matches page-fit renderers against the viewport's content box. */
export function fitDocumentPage(
  available: DocumentPageSize,
  natural: DocumentPageSize,
): DocumentPageSize | null {
  if (available.width <= 0 || available.height <= 0 || natural.width <= 0 || natural.height <= 0) return null;
  const scale = Math.min(available.width / natural.width, available.height / natural.height);
  return { width: natural.width * scale, height: natural.height * scale };
}

/** Matches fixed-width document renderers, which scroll instead of shrinking to viewport height. */
export function boundDocumentPageWidth(
  availableWidth: number,
  natural: DocumentPageSize,
  maximumWidth: number,
): DocumentPageSize | null {
  if (availableWidth <= 0 || natural.width <= 0 || natural.height <= 0) return null;
  const width = Math.min(availableWidth, maximumWidth);
  return { width, height: width * natural.height / natural.width };
}

/** Keeps PDF thumbnail and live canvases on the same CSS/backing-pixel transform. */
export function documentCanvasGeometry(
  natural: DocumentPageSize,
  cssScale: number,
  pixelRatio: number,
): DocumentCanvasGeometry | null {
  if (natural.width <= 0 || natural.height <= 0 || cssScale <= 0 || pixelRatio <= 0) return null;
  const renderScale = cssScale * pixelRatio;
  return {
    css: { width: natural.width * cssScale, height: natural.height * cssScale },
    pixels: {
      width: Math.ceil(natural.width * renderScale),
      height: Math.ceil(natural.height * renderScale),
    },
    renderScale,
  };
}
