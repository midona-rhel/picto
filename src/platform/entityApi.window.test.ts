import { afterEach, describe, expect, it, vi } from 'vitest';
import { queryItemWindow } from './entityApi';
import type { EntityViewQuery } from '../shared/types/canonical';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock('./ipc', () => ({ invoke: invokeMock }));
const query: EntityViewQuery = {
  scope: { kind: 'all' },
  view: { filter: { kind: 'all', value: [] }, sort: { field: 'name', direction: 'ascending', random_seed: null } },
};

afterEach(() => { vi.useRealTimers(); invokeMock.mockReset(); });

describe('prepared window deadline', () => {
  it('ends a stalled view and supersedes its backend generation', async () => {
    vi.useFakeTimers();
    invokeMock.mockImplementation((command: string) => command === 'items.window'
      ? new Promise(() => {}) : Promise.resolve());
    const pending = queryItemWindow(query, { position: { kind: 'fraction', value: 1 }, limit: 1500 },
      { channel: 'grid', generation: 7 });
    const failure = expect(pending).rejects.toThrow('Please retry');
    await vi.advanceTimersByTimeAsync(10_000);
    await failure;
    expect(invokeMock).toHaveBeenLastCalledWith('items.supersede_window', { channel: 'grid', generation: 8 });
    expect(vi.getTimerCount()).toBe(0);
  });

  it('cleans up the deadline when a coherent result arrives', async () => {
    vi.useFakeTimers();
    const result = { items: [], head_items: [], start: 0, total: 0, revision: 3 };
    invokeMock.mockResolvedValue(result);
    await expect(queryItemWindow(query, { position: { kind: 'ordinal', value: 0 }, limit: 500 })).resolves.toBe(result);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(invokeMock).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  });
});
