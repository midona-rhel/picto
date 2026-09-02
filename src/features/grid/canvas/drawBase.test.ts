import { describe, expect, it, vi } from 'vitest';
import { drawCanvasBaseLayer } from './drawBase';
import { drawBrokenThumbnail } from '../../../shared/ui/ThumbnailImage/drawBrokenThumbnail';

vi.mock('../../../shared/ui/ThumbnailImage/drawBrokenThumbnail', () => ({
  drawBrokenThumbnail: vi.fn(),
}));

describe('drawCanvasBaseLayer', () => {
  it('contains video thumbnails even when image thumbnails are configured to fill', () => {
    const drawImage = vi.fn();
    const ctx = {
      beginPath: vi.fn(),
      clip: vi.fn(),
      drawImage,
      fillRect: vi.fn(),
      filter: 'none',
      globalAlpha: 1,
      restore: vi.fn(),
      roundRect: vi.fn(),
      save: vi.fn(),
      stroke: vi.fn(),
    } as unknown as CanvasRenderingContext2D;
    const bitmap = { width: 200, height: 100 } as ImageBitmap;

    drawCanvasBaseLayer({
      ctx,
      positions: [{ x: 0, y: 0, w: 100, h: 100 }],
      items: [{
        itemId: 1,
        kind: 'media',
        displayFileHash: 'video',
        hash: '1',
        thumbnailHash: 'video',
        name: null,
        mime: 'video/webm',
        width: 200,
        height: 100,
        rating: null,
        durationMs: null,
        dominantColor: null,
        aspectRatio: 2,
        numFrames: null,
        mediaCount: 1,
      }],
      atlasGet: () => ({ thumb: bitmap, state: 'shown', lastAccessed: 0, bytes: 1 }),
      revealProgress: () => 1,
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

    expect(drawImage).toHaveBeenCalledWith(bitmap, 0, 0, 200, 100, 0, 25, 100, 50);
  });

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

  it('draws detailed broken artwork only inside the real viewport, not the overscan buffer', () => {
    vi.mocked(drawBrokenThumbnail).mockClear();
    const ctx = {
      beginPath: vi.fn(),
      clip: vi.fn(),
      fillRect: vi.fn(),
      filter: 'none',
      globalAlpha: 1,
      restore: vi.fn(),
      roundRect: vi.fn(),
      save: vi.fn(),
      stroke: vi.fn(),
    } as unknown as CanvasRenderingContext2D;
    const brokenItem = {
      itemId: 1,
      kind: 'media' as const,
      displayFileHash: 'missing',
      hash: '1',
      thumbnailHash: 'missing',
      name: null,
      mime: 'image/jpeg',
      width: 100,
      height: 100,
      rating: null,
      durationMs: null,
      dominantColor: null,
      aspectRatio: 1,
      numFrames: null,
      mediaCount: 1,
    };

    drawCanvasBaseLayer({
      ctx,
      positions: [
        { x: 0, y: 0, w: 100, h: 100 },
        { x: 0, y: 160, w: 100, h: 100 },
      ],
      items: [brokenItem, { ...brokenItem, itemId: 2, hash: '2' }],
      atlasGet: () => ({ thumb: null, state: 'error', lastAccessed: 0, bytes: 0 }),
      revealProgress: () => 0,
      activeTiles: [0, 1],
      draw: {
        scrollTop: 0,
        viewportHeight: 300,
        visibleScrollTop: 0,
        visibleViewportHeight: 100,
        textHeight: 0,
        borderRadius: 4,
      },
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

    expect(drawBrokenThumbnail).toHaveBeenCalledOnce();
  });
});
