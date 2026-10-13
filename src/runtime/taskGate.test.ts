import { act } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ForegroundTask } from '../shared/types/generated/application/ForegroundTask';
import { allowTaskAction, taskGate } from './taskGate';
import { applyTasks, taskPresentationAtom, tasksAtom } from './taskSettle';

vi.mock('../shared/lib/notifications', () => ({ showErrorNotification: vi.fn() }));

const store = getDefaultStore();

function task(overrides: Partial<ForegroundTask> = {}): ForegroundTask {
  return {
    task_id: 3,
    kind: 'import',
    title: 'Holiday',
    state: 'paused',
    scan_complete: true,
    total: 10,
    done: 4,
    added: 4,
    existing: 0,
    failed: 0,
    revertible: true,
    rate_per_second: null,
    eta_seconds: null,
    created_at_ms: 0,
    finished_at_ms: null,
    error: null,
    blocks: ['import'],
    ...overrides,
  };
}

beforeEach(() => {
  store.set(tasksAtom, []);
  store.set(taskPresentationAtom, { focusedTaskId: null, minimized: new Set([3]) });
});

describe('taskGate', () => {
  it('blocks what an open task names, and nothing once it ends', () => {
    expect(taskGate('import').blocked).toBe(false);
    act(() => applyTasks([task()]));
    expect(taskGate('import')).toMatchObject({ blocked: true, reason: 'Available when “Holiday” finishes' });
    act(() => applyTasks([task({ state: 'stopped', blocks: [] })]));
    expect(taskGate('import').blocked).toBe(false);
  });

  it('brings a minimized task in the way back to the front instead of starting', () => {
    act(() => applyTasks([task()]));
    store.set(taskPresentationAtom, { focusedTaskId: null, minimized: new Set([3]) });
    expect(allowTaskAction('import')).toBe(false);
    expect(store.get(taskPresentationAtom).focusedTaskId).toBe(3);
  });
});
