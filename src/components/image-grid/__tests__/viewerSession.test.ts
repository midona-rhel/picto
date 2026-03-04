/**
 * PBI-155: Unit tests for ViewerSession pure functions.
 *
 * Validates createSession, navigateSession, rebaseSession, and clampSession
 * which power the centralized viewer navigation model.
 */

import { describe, it, expect } from 'vitest';
import {
  createSession,
  navigateSession,
  rebaseSession,
  clampSession,
} from '../runtime/gridViewerSession';
import type { MasonryImageItem } from '../shared';

function makeImages(...hashes: string[]): MasonryImageItem[] {
  return hashes.map(hash => ({
    hash,
    name: hash,
    mime: 'image/jpeg',
    size: 1000,
    width: 100,
    height: 100,
    duration_ms: null,
    num_frames: null,
    has_audio: false,
    status: 'active',
    rating: null,
    view_count: 0,
    source_urls: null,
    imported_at: '2025-01-01T00:00:00Z',
    has_thumbnail: true,
    aspectRatio: 1,
  }));
}

describe('createSession', () => {
  it('creates session at the correct index', () => {
    const images = makeImages('a', 'b', 'c');
    const session = createSession(images, 'b');
    expect(session.currentIndex).toBe(1);
    expect(session.currentHash).toBe('b');
  });

  it('defaults to index 0 when hash not found', () => {
    const images = makeImages('a', 'b', 'c');
    const session = createSession(images, 'missing');
    expect(session.currentIndex).toBe(0);
    expect(session.currentHash).toBe('a');
  });

  it('handles empty images array', () => {
    const session = createSession([], 'anything');
    expect(session.currentIndex).toBe(0);
    expect(session.currentHash).toBe('anything');
  });
});

describe('navigateSession', () => {
  it('moves forward by delta', () => {
    const images = makeImages('a', 'b', 'c');
    const session = createSession(images, 'a');
    const next = navigateSession(session, images, 1);
    expect(next.currentIndex).toBe(1);
    expect(next.currentHash).toBe('b');
  });

  it('moves backward by delta', () => {
    const images = makeImages('a', 'b', 'c');
    const session = createSession(images, 'c');
    const next = navigateSession(session, images, -1);
    expect(next.currentIndex).toBe(1);
    expect(next.currentHash).toBe('b');
  });

  it('clamps at start', () => {
    const images = makeImages('a', 'b', 'c');
    const session = createSession(images, 'a');
    const next = navigateSession(session, images, -1);
    expect(next).toBe(session); // same reference — no change
  });

  it('clamps at end', () => {
    const images = makeImages('a', 'b', 'c');
    const session = createSession(images, 'c');
    const next = navigateSession(session, images, 1);
    expect(next).toBe(session); // same reference — no change
  });

  it('returns same reference for zero delta', () => {
    const images = makeImages('a', 'b', 'c');
    const session = createSession(images, 'b');
    const next = navigateSession(session, images, 0);
    expect(next).toBe(session);
  });

  it('handles empty images', () => {
    const session = { currentIndex: 0, currentHash: 'a' };
    const next = navigateSession(session, [], 1);
    expect(next).toBe(session);
  });
});

describe('rebaseSession', () => {
  it('rebases when hash moves to different index', () => {
    const session = { currentIndex: 1, currentHash: 'b' };
    const newImages = makeImages('x', 'y', 'b', 'z');
    const rebased = rebaseSession(session, newImages);
    expect(rebased).not.toBeNull();
    expect(rebased!.currentIndex).toBe(2);
    expect(rebased!.currentHash).toBe('b');
  });

  it('returns same reference when index unchanged', () => {
    const session = { currentIndex: 1, currentHash: 'b' };
    const newImages = makeImages('a', 'b', 'c');
    const rebased = rebaseSession(session, newImages);
    expect(rebased).toBe(session);
  });

  it('returns null when hash is gone', () => {
    const session = { currentIndex: 1, currentHash: 'b' };
    const newImages = makeImages('a', 'c', 'd');
    const rebased = rebaseSession(session, newImages);
    expect(rebased).toBeNull();
  });

  it('returns null for empty new images', () => {
    const session = { currentIndex: 0, currentHash: 'a' };
    const rebased = rebaseSession(session, []);
    expect(rebased).toBeNull();
  });
});

describe('clampSession', () => {
  it('clamps index to last item when list shrinks', () => {
    const session = { currentIndex: 5, currentHash: 'f' };
    const images = makeImages('a', 'b', 'c');
    const clamped = clampSession(session, images);
    expect(clamped.currentIndex).toBe(2);
    expect(clamped.currentHash).toBe('c');
  });

  it('no-ops when index is in bounds', () => {
    const images = makeImages('a', 'b', 'c');
    const session = { currentIndex: 1, currentHash: 'b' };
    const clamped = clampSession(session, images);
    expect(clamped.currentIndex).toBe(1);
    expect(clamped.currentHash).toBe('b');
  });

  it('handles single-item list', () => {
    const session = { currentIndex: 3, currentHash: 'd' };
    const images = makeImages('x');
    const clamped = clampSession(session, images);
    expect(clamped.currentIndex).toBe(0);
    expect(clamped.currentHash).toBe('x');
  });
});
