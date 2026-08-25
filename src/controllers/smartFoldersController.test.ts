import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { activeNodeIdAtom } from '../state/navigation';

const mocks = vi.hoisted(() => ({
  fetchTree: vi.fn().mockResolvedValue(undefined),
  loadFirstPage: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('./sidebarController', () => ({
  sidebarController: { fetchTree: mocks.fetchTree },
}));
vi.mock('./gridController', () => ({
  gridController: { loadFirstPage: mocks.loadFirstPage },
}));

import { smartFoldersController } from './smartFoldersController';

describe('smart folder refresh', () => {
  beforeEach(() => {
    mocks.fetchTree.mockClear();
    mocks.loadFirstPage.mockClear();
    getDefaultStore().set(activeNodeIdAtom, 'system:active');
  });

  it('refreshes counts without navigating to an inactive smart folder', async () => {
    await smartFoldersController.refresh(9);
    expect(mocks.fetchTree).toHaveBeenCalledOnce();
    expect(mocks.loadFirstPage).not.toHaveBeenCalled();
  });

  it('reruns the canonical query for the active smart folder', async () => {
    getDefaultStore().set(activeNodeIdAtom, 'smart:9');
    await smartFoldersController.refresh(9);
    expect(mocks.fetchTree).toHaveBeenCalledOnce();
    expect(mocks.loadFirstPage).toHaveBeenCalledWith({ preserveItems: true });
  });
});
