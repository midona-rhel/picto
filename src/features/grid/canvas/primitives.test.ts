import { describe, expect, it, vi } from 'vitest';
import {
  BADGE_PAD_X,
  INSPECTOR_BADGE_PAD_X,
  drawGroupBadge,
  formatLabelForMime,
} from './primitives';

describe('grid badge geometry', () => {
  it('uses the compact two-pixel horizontal inset', () => {
    expect(BADGE_PAD_X).toBe(2);
  });

  it('keeps only the inspector-style horizontal spacing distinct from the duration badge', () => {
    expect(INSPECTOR_BADGE_PAD_X).toBe(4);
  });

  it('renders the group glyph rather than a text abbreviation', () => {
    const ctx = {
      arcTo: vi.fn(),
      beginPath: vi.fn(),
      fill: vi.fn(),
      fillText: vi.fn(),
      lineTo: vi.fn(),
      moveTo: vi.fn(),
      restore: vi.fn(),
      roundRect: vi.fn(),
      save: vi.fn(),
      scale: vi.fn(),
      stroke: vi.fn(),
      translate: vi.fn(),
    } as unknown as CanvasRenderingContext2D;

    expect(drawGroupBadge(ctx, 5, 7)).toBe(18);
    expect(ctx.fillText).not.toHaveBeenCalled();
    expect(ctx.roundRect).toHaveBeenCalledTimes(2);
  });
});

describe('formatLabelForMime', () => {
  it.each([
    ['image/jpeg', 'JPG'],
    ['image/png', 'PNG'],
    ['video/mp4', 'MP4'],
    ['audio/flac', 'FLAC'],
    ['application/pdf', 'PDF'],
    ['text/markdown', 'MD'],
    ['application/vnd.openxmlformats-officedocument.wordprocessingml.document', 'DOCX'],
    ['application/x-shockwave-flash', 'SWF'],
    ['font/woff2', 'WOF2'],
  ])('maps %s to the compact %s label', (mime, label) => {
    expect(formatLabelForMime(mime)).toBe(label);
  });

  it('keeps JPGXL as the explicit five-character exception', () => {
    expect(formatLabelForMime('image/jxl')).toBe('JPGXL');
  });

  it('caps unknown MIME subtypes at four characters', () => {
    expect(formatLabelForMime('application/something-long')).toBe('SOME');
  });
});
