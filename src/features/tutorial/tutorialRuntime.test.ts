import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { startTutorialSession } from './tutorialRuntime';

describe('tutorial session startup', () => {
  const start = vi.fn();
  const finish = vi.fn();

  beforeEach(() => {
    start.mockResolvedValue({ path: '/tmp/Guided Tour.library' });
    finish.mockResolvedValue({ restored: true });
    Object.defineProperty(window, 'picto', {
      configurable: true,
      value: { tutorial: { start, finish } },
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
});
