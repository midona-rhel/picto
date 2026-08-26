import { describe, expect, it } from 'vitest';
import { boundDocumentPageWidth, documentCanvasGeometry, fitDocumentPage } from './documentPageGeometry';

describe('document page geometry', () => {
  it('fits tall pages from the available height without filling the width', () => {
    expect(fitDocumentPage(
      { width: 1200, height: 900 },
      { width: 600, height: 800 },
    )).toEqual({ width: 675, height: 900 });
  });

  it('keeps fixed-width documents bounded independently of viewport height', () => {
    expect(boundDocumentPageWidth(1200, { width: 816, height: 1056 }, 816)).toEqual({
      width: 816,
      height: 1056,
    });
  });

  it('uses one CSS and backing-pixel transform for document canvases', () => {
    expect(documentCanvasGeometry({ width: 612, height: 792 }, 1.25, 2)).toEqual({
      css: { width: 765, height: 990 },
      pixels: { width: 1530, height: 1980 },
      renderScale: 2.5,
    });
  });
});
