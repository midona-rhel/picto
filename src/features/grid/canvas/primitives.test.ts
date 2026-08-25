import { describe, expect, it } from 'vitest';
import {
  BADGE_PAD_X,
  INSPECTOR_BADGE_PAD_X,
  formatLabelForMime,
} from './primitives';

describe('grid badge geometry', () => {
  it('uses the compact two-pixel horizontal inset', () => {
    expect(BADGE_PAD_X).toBe(2);
  });

  it('keeps only the inspector-style horizontal spacing distinct from the duration badge', () => {
    expect(INSPECTOR_BADGE_PAD_X).toBe(4);
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
