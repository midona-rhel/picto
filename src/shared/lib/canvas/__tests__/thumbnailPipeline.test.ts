import { describe, expect, it } from 'vitest';

import { __private__ } from '../thumbnailPipeline';

describe('thumbnailPipeline source selection', () => {
  it('keeps thumbnail source for small tiles', () => {
    const request = __private__.buildRequest('abc', {
      y: 0,
      drawWidth: 240,
      drawHeight: 180,
      mime: 'image/jpeg',
      sourceWidth: 4000,
      sourceHeight: 3000,
    });

    expect(request?.sourceKind).toBe('thumbnail');
  });

  it('upgrades to full-quality source when tile exceeds thumbnail budget', () => {
    const request = __private__.buildRequest('abc', {
      y: 0,
      drawWidth: 900,
      drawHeight: 700,
      mime: 'image/jpeg',
      sourceWidth: 4000,
      sourceHeight: 3000,
    });

    expect(request?.sourceKind).toBe('full');
    expect(request?.resizeWidth).toBeGreaterThan(512);
  });
});
