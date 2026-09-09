import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { executeTutorialActions, finishTutorialSession, startTutorialSession } from './tutorialRuntime';
import { LIBRARY_OPEN_CANCELLED } from '../library/libraryOpenCancellation';

describe('tutorial session startup', () => {
  const start = vi.fn();
  const finish = vi.fn();
  const reset = vi.fn();
  const invoke = vi.fn();

  beforeEach(() => {
    start.mockResolvedValue({ path: '/tmp/Guided Tour.library' });
    finish.mockResolvedValue({ restored: true });
    reset.mockResolvedValue({ path: '/tmp/Guided Tour.library' });
    invoke.mockResolvedValue(null);
    Object.defineProperty(window, 'picto', {
      configurable: true,
      value: { tutorial: { start, finish, reset }, api: { invoke } },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
    vi.clearAllMocks();
  });

  it('continues when the remounted production sidebar is ready', async () => {
    const sidebar = document.createElement('aside');
    sidebar.dataset.helpId = 'sidebar';
    document.body.append(sidebar);

    await expect(startTutorialSession()).resolves.toBeUndefined();
    expect(start).toHaveBeenCalledOnce();
    expect(finish).not.toHaveBeenCalled();
  });

  it('restores the original library if the production shell never remounts', async () => {
    vi.useFakeTimers();
    const rejection = expect(startTutorialSession()).rejects.toThrow('timed out');
    await vi.advanceTimersByTimeAsync(20_100);

    await rejection;
    expect(finish).toHaveBeenCalledOnce();
  });

  it('surfaces a superseded or cancelled tour as the plain sentinel without finishing anything', async () => {
    start.mockRejectedValue(new Error(
      `Error invoking remote method 'picto:tutorial:start': Error: ${LIBRARY_OPEN_CANCELLED}`,
    ));

    await expect(startTutorialSession()).rejects.toThrow(new Error(LIBRARY_OPEN_CANCELLED));
    expect(finish).not.toHaveBeenCalled();
    // Nothing was captured for a tour that never opened.
    await expect(finishTutorialSession()).resolves.toBeUndefined();
    expect(finish).toHaveBeenCalledOnce();
  });

  it('surfaces a cancelled reset as the plain sentinel', async () => {
    invoke.mockImplementation(async (command: string) => (command === 'subscriptions.list'
      ? { subscriptions: [{ name: 'Leonardo da Vinci Archive', active_run_id: null, queries: [{ successful_run_count: 1 }] }] }
      : null));
    reset.mockRejectedValue(new Error(
      `Error invoking remote method 'picto:tutorial:reset': Error: ${LIBRARY_OPEN_CANCELLED}`,
    ));

    await expect(executeTutorialActions([{ type: 'set_tutorial_subscription_runs', count: 0 }]))
      .rejects.toThrow(new Error(LIBRARY_OPEN_CANCELLED));
    expect(reset).toHaveBeenCalledOnce();
  });
});
