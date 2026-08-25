import { describe, expect, it } from 'vitest';
import { detailRendererKind } from './detailRendererKind';

describe('detailRendererKind', () => {
  it.each([
    ['image/png', 'image'],
    ['image/jxl', 'jpeg-xl'],
    ['video/mp4', 'video'],
    ['audio/mpeg', 'audio'],
    ['application/pdf', 'pdf'],
    ['text/plain', 'text-document'],
    ['text/markdown', 'text-document'],
    ['application/json', 'text-document'],
    ['application/vnd.openxmlformats-officedocument.wordprocessingml.document', 'docx'],
    ['application/vnd.openxmlformats-officedocument.presentationml.presentation', 'pptx'],
    ['application/epub+zip', 'epub'],
    ['application/vnd.comicbook+zip', 'cbz'],
    ['image/vnd.djvu', 'djvu'],
    ['application/octet-stream', 'unsupported'],
    ['application/rtf', 'text-document'],
    ['application/x-shockwave-flash', 'flash'],
    ['font/ttf', 'font'],
    ['font/otf', 'font'],
    ['font/woff', 'font'],
    ['font/collection', 'font'],
  ] as const)('routes %s to the %s renderer', (mimeType, renderer) => {
    expect(detailRendererKind(mimeType)).toBe(renderer);
  });
});
