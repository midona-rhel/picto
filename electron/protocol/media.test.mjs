import { describe, expect, it } from 'vitest';
import { extToMime, isValidHash, parseMediaUrl, parseRange } from './media.mjs';

describe('media protocol helpers', () => {
  it('parses valid media URLs', () => {
    const hash = 'a'.repeat(64);
    expect(parseMediaUrl(`media://host/thumb/${hash}.jpg`)).toEqual({
      kind: 'thumb',
      hash,
      ext: 'jpg',
    });
    expect(parseMediaUrl(`media://host/file/${hash}.png`)).toEqual({
      kind: 'file',
      hash,
      ext: 'png',
    });
  });

  it('rejects invalid media URLs', () => {
    expect(parseMediaUrl('media://host/other/x')).toBeNull();
    expect(parseMediaUrl('media://host/file/not-a-hash.png')).toBeNull();
    expect(parseMediaUrl('media://host/thumb/abc.png')).toBeNull();
  });

  it('parses byte ranges', () => {
    expect(parseRange('bytes=0-99', 1000)).toEqual({ start: 0, end: 99 });
    expect(parseRange('bytes=100-', 1000)).toEqual({ start: 100, end: 999 });
    expect(parseRange('bytes=-50', 1000)).toEqual({ start: 950, end: 999 });
    expect(parseRange('bytes=2000-2100', 1000)).toBeNull();
  });

  it('maps mime types and validates hashes', () => {
    expect(extToMime('jpg')).toBe('image/jpeg');
    expect(extToMime('pdf')).toBe('application/pdf');
    expect(extToMime('weird')).toBe('application/octet-stream');
    expect(isValidHash('a'.repeat(64))).toBe(true);
    expect(isValidHash('z'.repeat(64))).toBe(false);
  });
});
