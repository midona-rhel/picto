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
    expect(request?.priority).toBe('visible');
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
    expect(request?.priority).toBe('visible');
    expect(request?.resizeWidth).toBeGreaterThan(512);
  });

  it('marks offscreen prefetch requests separately from visible work', () => {
    const request = __private__.buildRequest('abc', {
      y: 100,
    });

    expect(request?.sourceKind).toBe('thumbnail');
    expect(request?.priority).toBe('prefetch');
  });

  it('prioritizes visible thumbnails ahead of full-quality and prefetch work', () => {
    expect(__private__.scoreQueueItem({
      hash: 'visible-thumb',
      url: 'thumb://visible',
      y: 0,
      sourceKind: 'thumbnail',
      priority: 'visible',
      requestedLongEdge: 512,
      queuedAt: 0,
    })).toBeGreaterThan(__private__.scoreQueueItem({
      hash: 'visible-full',
      url: 'full://visible',
      y: 0,
      sourceKind: 'full',
      priority: 'visible',
      requestedLongEdge: 1024,
      queuedAt: 0,
    }));

    expect(__private__.scoreQueueItem({
      hash: 'visible-full',
      url: 'full://visible',
      y: 0,
      sourceKind: 'full',
      priority: 'visible',
      requestedLongEdge: 1024,
      queuedAt: 0,
    })).toBeGreaterThan(__private__.scoreQueueItem({
      hash: 'prefetch-thumb',
      url: 'thumb://prefetch',
      y: 2000,
      sourceKind: 'thumbnail',
      priority: 'prefetch',
      requestedLongEdge: 512,
      queuedAt: 0,
    }));
  });

  it('caps oversized full-quality requests at the display proxy budget', () => {
    expect(__private__.quantizeLongEdge(4096)).toBe(2048);
  });

  it('does not start prefetch work while scrolling', () => {
    expect(__private__.canStartQueueItem({
      item: {
        hash: 'prefetch-thumb',
        url: 'thumb://prefetch',
        y: 2000,
        sourceKind: 'thumbnail',
        priority: 'prefetch',
        requestedLongEdge: 512,
        queuedAt: 0,
      },
      budgets: __private__.getActiveBudgets('fast'),
      scrollPhase: 'fast',
      activeVisibleThumbLoads: 0,
      activePrefetchThumbLoads: 0,
      activeFullLoads: 0,

    })).toBe(false);
  });

  it('allows visible thumbnail work while scrolling within budget', () => {
    expect(__private__.canStartQueueItem({
      item: {
        hash: 'visible-thumb',
        url: 'thumb://visible',
        y: 0,
        sourceKind: 'thumbnail',
        priority: 'visible',
        requestedLongEdge: 512,
        queuedAt: 0,
      },
      budgets: __private__.getActiveBudgets('fast'),
      scrollPhase: 'fast',
      activeVisibleThumbLoads: 0,
      activePrefetchThumbLoads: 0,
      activeFullLoads: 0,

    })).toBe(true);
  });

  it('allows full-quality upgrades within budget regardless of scroll phase', () => {
    expect(__private__.canStartQueueItem({
      item: {
        hash: 'visible-full',
        url: 'full://visible',
        y: 0,
        sourceKind: 'full',
        priority: 'visible',
        requestedLongEdge: 1024,
        queuedAt: 0,
      },
      budgets: __private__.getActiveBudgets('fast'),
      scrollPhase: 'fast',
      activeVisibleThumbLoads: 0,
      activePrefetchThumbLoads: 0,
      activeFullLoads: 0,

    })).toBe(true);
  });

  it('treats prefetch work as non-visible work during scrolling', () => {
    expect(__private__.canStartQueueItem({
      item: {
        hash: 'prefetch-thumb',
        url: 'thumb://prefetch',
        y: 4000,
        sourceKind: 'thumbnail',
        priority: 'prefetch',
        requestedLongEdge: 512,
        queuedAt: 0,
      },
      budgets: __private__.getActiveBudgets('fast'),
      scrollPhase: 'fast',
      activeVisibleThumbLoads: 0,
      activePrefetchThumbLoads: 1,
      activeFullLoads: 0,

    })).toBe(false);
  });

  it('allows a small prefetch lane during slow scrolling', () => {
    expect(__private__.canStartQueueItem({
      item: {
        hash: 'prefetch-thumb',
        url: 'thumb://prefetch',
        y: 1200,
        sourceKind: 'thumbnail',
        priority: 'prefetch',
        requestedLongEdge: 512,
        queuedAt: 0,
      },
      budgets: __private__.getActiveBudgets('slow'),
      scrollPhase: 'slow',
      activeVisibleThumbLoads: 0,
      activePrefetchThumbLoads: 0,
      activeFullLoads: 0,

    })).toBe(true);
  });

  it('allows full-quality upgrades during slow scrolling when visible thumbnails are clear', () => {
    expect(__private__.canStartQueueItem({
      item: {
        hash: 'visible-full',
        url: 'full://visible',
        y: 0,
        sourceKind: 'full',
        priority: 'visible',
        requestedLongEdge: 1024,
        queuedAt: 0,
      },
      budgets: __private__.getActiveBudgets('slow'),
      scrollPhase: 'slow',
      activeVisibleThumbLoads: 0,
      activePrefetchThumbLoads: 0,
      activeFullLoads: 0,

    })).toBe(true);
  });
});
