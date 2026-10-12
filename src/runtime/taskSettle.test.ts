import { act } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ForegroundTask } from '../shared/types/generated/application/ForegroundTask';

const mocks = vi.hoisted(() => ({
  listTasks: vi.fn(),
  callbacks: new Map<string, () => void>(),
}));

vi.mock('../platform/taskApi', () => ({ listTasks: mocks.listTasks }));
vi.mock('./libraryInvalidation', () => ({
  libraryInvalidation: {
    register: (resource: string, callback: () => void) => {
      mocks.callbacks.set(resource, callback);
      return () => mocks.callbacks.delete(resource);
    },
  },
}));

import { applyTasks, startTaskSettle, taskPresentationAtom, tasksAtom } from './taskSettle';

const store = getDefaultStore();

function task(id: number, state: string): ForegroundTask {
  return {
    task_id: id, kind: 'import', title: `Import ${id}`, state, scan_complete: true, total: 10, done: 1,
    added: 1, existing: 0, failed: 0, revertible: true, rate_per_second: null, eta_seconds: null,
    created_at_ms: 0, finished_at_ms: null, error: null,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  store.set(tasksAtom, []);
  store.set(taskPresentationAtom, { focusedTaskId: null, minimized: new Set<number>() });
  mocks.listTasks.mockReset();
  mocks.callbacks.clear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('task settle', () => {
  it('brings a task resumed after reopening to the foreground', () => {
    applyTasks([task(1, 'finished'), task(2, 'paused')]);
    expect(store.get(taskPresentationAtom).focusedTaskId).toBe(2);
  });

  it('polls while a task works and stops polling once it ends', async () => {
    mocks.listTasks.mockResolvedValueOnce([task(1, 'running')]).mockResolvedValueOnce([task(1, 'finished')]);
    const stop = startTaskSettle();
    await act(async () => { await Promise.resolve(); });
    expect(store.get(tasksAtom)[0].state).toBe('running');

    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(mocks.listTasks).toHaveBeenCalledTimes(2);
    expect(store.get(tasksAtom)[0].state).toBe('finished');

    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(mocks.listTasks).toHaveBeenCalledTimes(2);
    stop();
  });

  it('coalesces a burst of task changes into one read', async () => {
    mocks.listTasks.mockResolvedValue([]);
    const stop = startTaskSettle();
    await act(async () => { await Promise.resolve(); });
    for (let index = 0; index < 5; index += 1) mocks.callbacks.get('tasks')?.();
    await act(async () => { await vi.advanceTimersByTimeAsync(200); });
    expect(mocks.listTasks).toHaveBeenCalledTimes(2);
    stop();
  });
});
