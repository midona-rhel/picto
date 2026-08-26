import { describe, expect, it, vi } from 'vitest';
import { drawCanvasBaseLayer } from './drawBase';

vi.mock('../../../shared/ui/ThumbnailImage/drawBrokenThumbnail', () => ({
  drawBrokenThumbnail: vi.fn(),
}));

describe('drawCanvasBaseLayer', () => {
  it('uses a neutral surface behind broken media instead of its dominant color', () => {
    const fills: string[] = [];
    const ctx = {
      beginPath: vi.fn(),
      clip: vi.fn(),
      fillRect: vi.fn(function (this: { fillStyle: string }) {
        fills.push(this.fillStyle);
      }),
      filter: 'none',
      globalAlpha: 1,
      restore: vi.fn(),
      roundRect: vi.fn(),
      save: vi.fn(),
      stroke: vi.fn(),
    } as unknown as CanvasRenderingContext2D;

    drawCanvasBaseLayer({
      ctx,
      positions: [{ x: 0, y: 0, w: 100, h: 100 }],
      items: [{
        itemId: 1,
        kind: 'media',
        displayFileHash: 'missing',
        hash: '1',
        thumbnailHash: 'missing',
        name: null,
        mime: 'image/jpeg',
        width: 100,
        height: 100,
        rating: null,
        durationMs: null,
        dominantColor: '#ff0000',
        aspectRatio: 1,
        numFrames: null,
        mediaCount: 1,
      }],
      atlasGet: () => ({ thumb: null, state: 'error', lastAccessed: 0, bytes: 0 }),
      revealProgress: () => 0,
      activeTiles: [0],
      draw: { scrollTop: 0, viewportHeight: 100, textHeight: 0, borderRadius: 4 },
      theme: {
        placeholderBg: '#202124',
        isLight: false,
        borderRadius: 4,
        textPrimary: '#fff',
        textTertiary: '#aaa',
        glassBorder: '#444',
      },
      viewMode: 'grid',
      fitThumbnails: true,
      grayscale: false,
      showTileName: false,
      showResolution: false,
      showExtension: false,
      showExtensionLabel: false,
      showItemCount: false,
    });

    expect(fills).toEqual(['#202124']);
  });
});
