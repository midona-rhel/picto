import { beforeEach, describe, expect, it, vi } from 'vitest';

const callbacks = new Map<string, () => void>();
const fetchTree = vi.hoisted(() => vi.fn());
const start = vi.hoisted(() => vi.fn());

vi.mock('./libraryInvalidation', () => ({
  libraryInvalidation: {
    register: vi.fn((resource: string, callback: () => void) => {
      callbacks.set(resource, callback);
      return () => callbacks.delete(resource);
    }),
    start,
  },
}));

vi.mock('../controllers/sidebarController', () => ({ sidebarController: { fetchTree } }));

import { startSidebarSettle } from './sidebarSettle';

describe('sidebar runtime settling', () => {
  beforeEach(() => {
    callbacks.clear();
    fetchTree.mockReset().mockResolvedValue(undefined);
    start.mockReset();
  });

  it('subscribes to every resource that changes sidebar counts', () => {
    const stop = startSidebarSettle();

    expect([...callbacks.keys()]).toEqual(['sidebar', 'folders', 'smart_folders', 'tags']);
    expect(start).not.toHaveBeenCalled();

    stop();
    expect(callbacks.size).toBe(0);
  });

  it('refreshes canonical navigation and counts for each invalidated resource', async () => {
    const stop = startSidebarSettle();

    callbacks.get('sidebar')?.();
    callbacks.get('folders')?.();
    callbacks.get('smart_folders')?.();
    callbacks.get('tags')?.();
    await Promise.resolve();

    expect(fetchTree).toHaveBeenCalledTimes(4);
    stop();
  });
});
