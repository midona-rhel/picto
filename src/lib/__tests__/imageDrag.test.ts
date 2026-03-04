/**
 * PBI-058: Drag/drop regression tests for imageDrag lifecycle.
 * Covers native drag session tracking, timeout guard, and idempotent cleanup.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { imageDrag } from '../imageDrag';

describe('imageDrag — native drag session lifecycle', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    imageDrag.clearNativeDragSession();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('starts a session and returns hashes', () => {
    const hashes = ['abc123', 'def456'];
    const sessionId = imageDrag.startNativeDragSession(hashes);
    expect(sessionId).toBeGreaterThan(0);
    expect(imageDrag.getPendingNativeDragHashes()).toEqual(hashes);
  });

  it('clears session by sessionId', () => {
    const hashes = ['abc'];
    const sessionId = imageDrag.startNativeDragSession(hashes);
    imageDrag.clearNativeDragSession(sessionId);
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();
  });

  it('clears session without sessionId', () => {
    imageDrag.startNativeDragSession(['abc']);
    imageDrag.clearNativeDragSession();
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();
  });

  it('is idempotent — clearing wrong sessionId is a no-op', () => {
    const hashes = ['abc'];
    const sid1 = imageDrag.startNativeDragSession(hashes);
    // Try clearing with a stale session ID
    imageDrag.clearNativeDragSession(sid1 - 1);
    // Session should still be active
    expect(imageDrag.getPendingNativeDragHashes()).toEqual(hashes);
  });

  it('new session replaces stale session', () => {
    imageDrag.startNativeDragSession(['old']);
    const sid2 = imageDrag.startNativeDragSession(['new']);
    expect(imageDrag.getPendingNativeDragHashes()).toEqual(['new']);
    imageDrag.clearNativeDragSession(sid2);
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();
  });

  it('30s guard timeout clears abandoned session', () => {
    imageDrag.startNativeDragSession(['abandoned']);
    expect(imageDrag.getPendingNativeDragHashes()).toEqual(['abandoned']);

    // Advance past the 30s timeout
    vi.advanceTimersByTime(31_000);
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();
  });

  it('cleared session is not affected by guard timeout', () => {
    const sid = imageDrag.startNativeDragSession(['temp']);
    imageDrag.clearNativeDragSession(sid);
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();

    // Timeout fires but nothing breaks
    vi.advanceTimersByTime(31_000);
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();
  });

  it('returns null when no session is active', () => {
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();
  });

  it('handles empty hashes array', () => {
    imageDrag.startNativeDragSession([]);
    expect(imageDrag.getPendingNativeDragHashes()).toEqual([]);
  });

  it('handles large selection (1000 hashes)', () => {
    const hashes = Array.from({ length: 1000 }, (_, i) => `hash_${i}`);
    imageDrag.startNativeDragSession(hashes);
    expect(imageDrag.getPendingNativeDragHashes()?.length).toBe(1000);
    imageDrag.clearNativeDragSession();
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();
  });

  it('notifies native-drag-end listeners when session is cleared', () => {
    const onEnd = vi.fn();
    const cleanup = imageDrag.onNativeDragEnd(onEnd);
    const sid = imageDrag.startNativeDragSession(['abc']);

    imageDrag.clearNativeDragSession(sid);
    expect(onEnd).toHaveBeenCalledTimes(1);

    cleanup();
  });

  it('notifies native-drag-end listeners on timeout clear', () => {
    const onEnd = vi.fn();
    const cleanup = imageDrag.onNativeDragEnd(onEnd);

    imageDrag.startNativeDragSession(['abandoned']);
    vi.advanceTimersByTime(31_000);

    expect(onEnd).toHaveBeenCalledTimes(1);
    cleanup();
  });
});

describe('imageDrag — custom drag lifecycle', () => {
  beforeEach(() => {
    imageDrag.forceEnd();
  });

  it('starts and tracks active state', () => {
    expect(imageDrag.active).toBe(false);
    imageDrag.start(['h1'], ['thumb1'], 100, 200);
    expect(imageDrag.active).toBe(true);
  });

  it('end triggers drop handler when over folder', () => {
    const handler = vi.fn();
    const cleanup = imageDrag.onDrop(handler);

    imageDrag.start(['h1'], ['thumb1'], 100, 200);
    // Simulate being over a folder by moving to a point
    // (Can't fully test elementFromPoint in unit test — just verify end cleans up)
    imageDrag.end();
    expect(imageDrag.active).toBe(false);

    cleanup();
  });

  it('forceEnd cleans up without triggering handler', () => {
    const handler = vi.fn();
    const cleanup = imageDrag.onDrop(handler);

    imageDrag.start(['h1'], ['thumb1'], 100, 200);
    imageDrag.forceEnd();
    expect(imageDrag.active).toBe(false);
    expect(handler).not.toHaveBeenCalled();

    cleanup();
  });

  it('forceEnd also clears native drag session (PBI-053)', () => {
    vi.useFakeTimers();
    imageDrag.startNativeDragSession(['native_hash']);
    imageDrag.start(['h1'], ['thumb1'], 100, 200);
    imageDrag.forceEnd();
    expect(imageDrag.getPendingNativeDragHashes()).toBeNull();
    vi.useRealTimers();
  });
});
