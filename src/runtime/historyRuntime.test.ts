import { beforeEach, describe, expect, it, vi } from 'vitest';
import { clearNotifications, getCurrentNotification } from '../shared/lib/notifications';

const mocks = vi.hoisted(() => ({
  handlers: new Map<string, () => void>(),
  state: vi.fn(),
  undo: vi.fn(),
  redo: vi.fn(),
}));

vi.mock('../platform/ipc', () => ({
  listen: vi.fn(async (name: string, handler: () => void) => {
    mocks.handlers.set(name, handler);
    return () => mocks.handlers.delete(name);
  }),
}));

vi.mock('../platform/historyApi', () => ({
  getHistoryState: mocks.state,
  undoHistory: mocks.undo,
  redoHistory: mocks.redo,
}));

import {
  announceUndoableMutation,
  resetHistoryRuntimeForTests,
  startHistoryRuntime,
} from './historyRuntime';

const renameEntry = { entry_id: 4, command: 'items.rename', label: 'Rename item' };

describe('historyRuntime', () => {
  beforeEach(() => {
    clearNotifications();
    resetHistoryRuntimeForTests();
    mocks.handlers.clear();
    mocks.state.mockReset();
    mocks.undo.mockReset();
    mocks.redo.mockReset();
  });

  it('announces only the mutation that owns the newest history entry', async () => {
    mocks.state.mockResolvedValue({ undo: renameEntry, redo: null });
    await announceUndoableMutation('items.rename');
    expect(getCurrentNotification()?.title).toBe('Rename item');
    expect(getCurrentNotification()?.action?.label).toBe('Undo');

    clearNotifications();
    await announceUndoableMutation('items.patch_metadata');
    expect(getCurrentNotification()).toBeNull();
  });

  it('uses one menu and notification path for undo and redo', async () => {
    mocks.undo.mockResolvedValue({
      entry: renameEntry,
      state: { undo: null, redo: renameEntry },
      receipt: { revision: 2, resources: ['library'], item_ids: [1] },
    });
    mocks.redo.mockResolvedValue({
      entry: renameEntry,
      state: { undo: renameEntry, redo: null },
      receipt: { revision: 3, resources: ['library'], item_ids: [1] },
    });
    const stop = startHistoryRuntime();
    await vi.waitFor(() => expect(mocks.handlers.has('menu:undo')).toBe(true));

    mocks.handlers.get('menu:undo')?.();
    await vi.waitFor(() => expect(mocks.undo).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(getCurrentNotification()?.action?.label).toBe('Redo'));

    getCurrentNotification()?.action?.onClick();
    await vi.waitFor(() => expect(mocks.redo).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(getCurrentNotification()?.action?.label).toBe('Undo'));
    stop();
  });

  it('keeps editable-field history native', async () => {
    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();
    const exec = vi.fn(() => true);
    Object.defineProperty(document, 'execCommand', { configurable: true, value: exec });
    const stop = startHistoryRuntime();
    await vi.waitFor(() => expect(mocks.handlers.has('menu:undo')).toBe(true));

    mocks.handlers.get('menu:undo')?.();
    expect(exec).toHaveBeenCalledWith('undo');
    expect(mocks.undo).not.toHaveBeenCalled();
    stop();
    input.remove();
    Reflect.deleteProperty(document, 'execCommand');
  });
});
