import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { stopAiTaggerTasksForTests, useAiTaggerTasks } from './aiTaggerTasks';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  onChange: undefined as ((payload: any) => void) | undefined,
}));

vi.mock('../platform/ipc', () => ({
  invoke: mocks.invoke,
  listen: mocks.listen,
}));

const taskSnapshot = { ingest: { pending: 0, running: 0, failed: 0 }, background: { pending: 1, running: 0, failed: 0 }, subscriptions: { pending: 0, running: 0, failed: 0 }, issues: [], revision: 1 };

describe('AI task reader', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.invoke.mockResolvedValue(taskSnapshot);
    mocks.listen.mockImplementation(async (_name: string, handler: (event: { payload: unknown }) => void) => {
      mocks.onChange = (payload) => handler({ payload });
      return () => { mocks.onChange = undefined; };
    });
  });

  afterEach(() => stopAiTaggerTasksForTests());

  it('reads persisted task state and refreshes only for task invalidation', async () => {
    const { result } = renderHook(() => useAiTaggerTasks());
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('tasks.get'));
    expect(result.current.snapshot).toEqual(taskSnapshot);

    mocks.invoke.mockResolvedValue({ ...taskSnapshot, revision: 2 });
    act(() => mocks.onChange?.({ revision: 2, resources: ['tasks'], item_ids: [] }));
    await waitFor(() => expect(result.current.snapshot?.revision).toBe(2));

    const calls = mocks.invoke.mock.calls.length;
    act(() => mocks.onChange?.({ revision: 3, resources: ['tags'], item_ids: [] }));
    await Promise.resolve();
    expect(mocks.invoke).toHaveBeenCalledTimes(calls);
  });
});
