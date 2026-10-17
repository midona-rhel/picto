import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createInboxBadgeService } from './inboxBadgeService.mjs';

function harness(initialCount = 0) {
  let count = initialCount;
  let root = '/library';
  const app = { setBadgeCount: vi.fn() };
  const invokeSerialized = vi.fn(async (command) => {
    expect(command).toBe('library.inbox_count');
    return JSON.stringify(count);
  });
  const service = createInboxBadgeService({ app, invokeSerialized, getCurrentLibraryRoot: () => root });
  return {
    app,
    invokeSerialized,
    service,
    setCount: (next) => { count = next; },
    setRoot: (next) => { root = next; },
  };
}

describe('inbox badge', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('shows the Inbox count and follows library changes, up and down', async () => {
    const { app, service, setCount } = harness(12);
    await service.refresh();
    expect(app.setBadgeCount).toHaveBeenLastCalledWith(12);

    setCount(15);
    service.handleNativeEvent('library/changed');
    await vi.advanceTimersByTimeAsync(200);
    expect(app.setBadgeCount).toHaveBeenLastCalledWith(15);

    setCount(0);
    service.handleNativeEvent('library/changed');
    await vi.advanceTimersByTimeAsync(200);
    expect(app.setBadgeCount).toHaveBeenLastCalledWith(0);
  });

  it('reads once for a burst of changes and skips unchanged counts', async () => {
    const { app, invokeSerialized, service } = harness(4);
    await service.refresh();
    for (let index = 0; index < 10; index += 1) service.handleNativeEvent('library/changed');
    service.handleNativeEvent('picto:thumbnail-changed');
    await vi.advanceTimersByTimeAsync(200);
    expect(invokeSerialized).toHaveBeenCalledTimes(2);
    expect(app.setBadgeCount).toHaveBeenCalledTimes(1);
  });

  it('clears when no library is open, and caps large counts', async () => {
    const { app, service, setCount, setRoot } = harness();
    setCount(20_000);
    await service.refresh();
    expect(app.setBadgeCount).toHaveBeenLastCalledWith(9_999);
    setRoot(null);
    await service.refresh();
    expect(app.setBadgeCount).toHaveBeenLastCalledWith(0);
  });
});
