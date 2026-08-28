import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { gridSelectionAtom } from '../state/selection';
import { exportModalAtom } from '../state/modals';

const mocks = vi.hoisted(() => ({
  listeners: new Map<string, () => void>(),
  chooseAndImportFiles: vi.fn().mockResolvedValue(undefined),
  chooseAndImportFolder: vi.fn().mockResolvedValue(undefined),
  exportMedia: vi.fn().mockResolvedValue(undefined),
  showError: vi.fn(),
  showInfo: vi.fn(),
  showSuccess: vi.fn(),
}));

vi.mock('../platform/ipc', () => ({
  listen: vi.fn((name: string, handler: () => void) => {
    mocks.listeners.set(name, handler);
    return Promise.resolve(vi.fn());
  }),
}));
vi.mock('../controllers/filesController', () => ({
  chooseAndImportFiles: mocks.chooseAndImportFiles,
  chooseAndImportFolder: mocks.chooseAndImportFolder,
  filesController: { exportMedia: mocks.exportMedia },
}));
vi.mock('../shared/lib/notifications', () => ({
  showErrorNotification: mocks.showError,
  showInfoNotification: mocks.showInfo,
  showSuccessNotification: mocks.showSuccess,
}));

import { startApplicationMenuRuntime } from './applicationMenuRuntime';

const store = getDefaultStore();

describe('application menu runtime', () => {
  beforeEach(() => {
    mocks.listeners.clear();
    store.set(gridSelectionAtom, {
      mode: 'explicit',
      itemIds: new Set([7]),
      excludedItemIds: new Set<number>(),
      folderNodeIds: new Set<string>(),
      anchor: { kind: 'item', id: 7 },
    });
    store.set(exportModalAtom, { open: false, fileCount: 0 });
    (window as any).picto = {
      dialog: { open: vi.fn().mockResolvedValue('/tmp/export') },
    };
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('connects native imports and advanced export to the existing renderer workflows', async () => {
    const stop = startApplicationMenuRuntime();
    mocks.listeners.get('menu:import-files')?.();
    mocks.listeners.get('menu:import-folder')?.();
    mocks.listeners.get('menu:export-advanced')?.();
    await vi.waitFor(() => {
      expect(mocks.chooseAndImportFiles).toHaveBeenCalledOnce();
      expect(mocks.chooseAndImportFolder).toHaveBeenCalledOnce();
    });
    expect(store.get(exportModalAtom)).toEqual({
      open: true,
      fileCount: 1,
      target: { kind: 'explicit', root_ids: [7] },
    });
    stop();
  });

  it('exports originals directly through the existing export controller', async () => {
    startApplicationMenuRuntime();
    mocks.listeners.get('menu:export-basic')?.();
    await vi.waitFor(() => expect(mocks.exportMedia).toHaveBeenCalledWith(
      { kind: 'explicit', root_ids: [7] },
      { output_dir: '/tmp/export', format: 'original' },
    ));
  });
});
