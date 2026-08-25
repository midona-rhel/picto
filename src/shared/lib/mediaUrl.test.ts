import { describe, expect, it } from 'vitest';
import { mediaFileUrl, mimeToMediaExt } from './mediaUrl';

describe('media URLs', () => {
  it.each([
    ['audio/mpeg', 'mp3'],
    ['audio/mp4', 'm4a'],
    ['audio/flac', 'flac'],
    ['audio/ogg', 'ogg'],
    ['audio/wav', 'wav'],
  ])('maps %s to its playable extension', (mime, extension) => {
    expect(mimeToMediaExt(mime)).toBe(extension);
    expect(mediaFileUrl('hash', mime)).toBe(`media://localhost/file/hash.${extension}`);
  });

  it.each([
    ['application/pdf', 'pdf'],
    ['image/jxl', 'jxl'],
    ['application/vnd.openxmlformats-officedocument.wordprocessingml.document', 'docx'],
    ['application/vnd.openxmlformats-officedocument.presentationml.presentation', 'pptx'],
    ['application/epub+zip', 'epub'],
    ['application/vnd.comicbook+zip', 'cbz'],
    ['image/vnd.djvu', 'djvu'],
    ['application/x-shockwave-flash', 'swf'],
    ['font/otf', 'otf'],
  ])('maps %s to its original document extension', (mime, extension) => {
    expect(mimeToMediaExt(mime)).toBe(extension);
    expect(mediaFileUrl('hash', mime)).toBe(`media://localhost/file/hash.${extension}`);
  });
});
