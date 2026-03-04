import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invalidateMock = vi.fn();
const fetchSidebarTreeMock = vi.fn(async () => {});

vi.mock('../../stores/domainStore', () => ({
  useDomainStore: {
    getState: () => ({
      invalidate: invalidateMock,
      fetchSidebarTree: fetchSidebarTreeMock,
    }),
  },
}));

import { SidebarController } from '../sidebarController';

describe('SidebarController', () => {
  beforeEach(() => {
    invalidateMock.mockReset();
    fetchSidebarTreeMock.mockReset();
    vi.useFakeTimers();
    // Force timer path so tests are deterministic under jsdom.
    Object.defineProperty(window, 'requestAnimationFrame', {
      configurable: true,
      writable: true,
      value: undefined,
    });
  });

  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it('coalesces repeated requestRefresh calls into one invalidate tick', () => {
    SidebarController.requestRefresh();
    SidebarController.requestRefresh();
    SidebarController.requestRefresh();

    expect(invalidateMock).not.toHaveBeenCalled();
    vi.runOnlyPendingTimers();
    expect(invalidateMock).toHaveBeenCalledTimes(1);

    SidebarController.requestRefresh();
    vi.runOnlyPendingTimers();
    expect(invalidateMock).toHaveBeenCalledTimes(2);
  });

  it('delegates initial fetch to domain store', async () => {
    await SidebarController.fetchInitialTree();
    expect(fetchSidebarTreeMock).toHaveBeenCalledTimes(1);
  });
});
