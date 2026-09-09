import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { gridController } from '../controllers/gridController';
import { gridSessionAtom, gridTransitionPhaseAtom } from '../state/grid';
import { startGridSettle } from './gridSettle';
import { labToHex } from '../shared/lib/labColor';
import type { CanonicalEntityGridItem } from '../shared/types/canonical';

const { callbacks, derivative, queryMock } = vi.hoisted(() => ({
  callbacks: new Map<string, () => void>(), queryMock: vi.fn(),
  derivative: { onColor: undefined as undefined | ((payload: { fileHash: string; dominantColorHex: string | null }) => void) },
}));
vi.mock('../platform/entityApi', () => ({ queryItemWindow: queryMock, supersedeItemWindow: vi.fn().mockResolvedValue(undefined) }));
vi.mock('./libraryInvalidation', () => ({ libraryInvalidation: {
  latestRevision: () => -Infinity,
  register: (resource: string, callback: () => void) => {
    callbacks.set(resource, callback); return () => callbacks.delete(resource);
  },
} }));
vi.mock('../shared/lib/thumbnailChanges', () => ({
  listenDominantColorChanged: vi.fn(callback => {
    derivative.onColor = callback;
    return Promise.resolve(() => { derivative.onColor = undefined; });
  }),
}));
const store = getDefaultStore();
const item = (id: number): CanonicalEntityGridItem => ({
  root_id: id, kind: 'media', lifecycle: 'active', name: 'Image ' + id, cover_media_id: id,
  content_hash: 'hash-' + id, mime: 'image/jpeg', width: 100, height: 100,
  duration_ms: null, frame_count: null, palette: [], imported_at_ms: 1, captured_at_ms: null,
  modified_at_ms: 1, rating: 'unrated', media_count: 1, total_size_bytes: 100,
});
const result = (id: number, revision = 2) => ({
  items: [item(id)], head_items: [item(id)], start: 0, total: 1,
  media_count: 1, total_size_bytes: 100, next_cursor: null, revision,
});
let stop: (() => void) | undefined;
beforeEach(() => {
  vi.useFakeTimers();
  gridController.deactivate();
  queryMock.mockReset().mockResolvedValue(result(2));
  store.set(gridTransitionPhaseAtom, 'idle');
  store.set(gridSessionAtom, { ...store.get(gridSessionAtom), scope: { kind: 'all' },
    active: true, status: 'idle', items: [item(1)], queryHeadItems: [item(1)], windowStart: 0,
    totalCount: 1, revision: 1, error: null });
  stop = startGridSettle();
});
afterEach(() => {
  stop?.(); gridController.deactivate(); callbacks.clear(); derivative.onColor = undefined;
  vi.useRealTimers();
});
describe('grid invalidation', () => {
  it('keeps the displayed grid until a coherent refresh replaces it', async () => {
    callbacks.get('library')?.();
    expect(store.get(gridSessionAtom).items[0].root_id).toBe(1);
    await vi.advanceTimersByTimeAsync(40);
    expect(store.get(gridSessionAtom).items[0].root_id).toBe(2);
    expect(store.get(gridSessionAtom).queryHeadItems[0].root_id).toBe(2);
    expect(store.get(gridSessionAtom).status).toBe('idle');
  });
  it('ignores recent-view invalidation outside the recent scope', async () => {
    callbacks.get('recently_viewed')?.();
    await vi.advanceTimersByTimeAsync(40);
    expect(queryMock).not.toHaveBeenCalled();
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), scope: { kind: 'recently_viewed' } });
    callbacks.get('recently_viewed')?.();
    await vi.advanceTimersByTimeAsync(40);
    expect(store.get(gridSessionAtom).items[0].root_id).toBe(2);
  });
  it('retains invalidations through a transition and refreshes after commit', async () => {
    store.set(gridTransitionPhaseAtom, 'waiting');
    callbacks.get('library')?.(); callbacks.get('library')?.();
    await vi.advanceTimersByTimeAsync(2000);
    expect(queryMock).not.toHaveBeenCalled();
    store.set(gridTransitionPhaseAtom, 'fading_in');
    await vi.advanceTimersByTimeAsync(40);
    expect(queryMock).toHaveBeenCalledOnce();
    expect(store.get(gridSessionAtom).items[0].root_id).toBe(2);
  });
  it('does not query while the grid is inactive', async () => {
    gridController.deactivate(); callbacks.get('library')?.();
    await vi.advanceTimersByTimeAsync(2000);
    expect(queryMock).not.toHaveBeenCalled();
  });
  it('does not let an already scheduled refresh supersede destination preparation', async () => {
    callbacks.get('library')?.();
    store.set(gridTransitionPhaseAtom, 'fading_out');
    await vi.advanceTimersByTimeAsync(40);
    expect(queryMock).not.toHaveBeenCalled();
    store.set(gridTransitionPhaseAtom, 'waiting');
    await vi.advanceTimersByTimeAsync(2000);
    expect(queryMock).not.toHaveBeenCalled();
    store.set(gridTransitionPhaseAtom, 'fading_in');
    await vi.advanceTimersByTimeAsync(40);
    expect(queryMock).toHaveBeenCalledOnce();
  });
  it('settles the current snapshot before one trailing refresh during a write burst', async () => {
    let finish: ((value: ReturnType<typeof result>) => void) | undefined;
    queryMock.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
      .mockResolvedValueOnce(result(3, 3));
    callbacks.get('library')?.(); await vi.advanceTimersByTimeAsync(40);
    callbacks.get('library')?.(); callbacks.get('library')?.();
    expect(queryMock).toHaveBeenCalledOnce();
    finish?.(result(2)); await vi.advanceTimersByTimeAsync(0);
    expect(store.get(gridSessionAtom).items[0].root_id).toBe(2);
    await vi.advanceTimersByTimeAsync(2000);
    expect(queryMock).toHaveBeenCalledTimes(2);
    expect(store.get(gridSessionAtom).items[0].root_id).toBe(3);
  });
  it('reveals a completed dominant color without replacing the grid', () => {
    derivative.onColor?.({ fileHash: 'hash-1', dominantColorHex: '#123456' });
    expect(labToHex(store.get(gridSessionAtom).items[0].palette[0])).toBe('#123456');
    expect(queryMock).not.toHaveBeenCalled();
  });
});
