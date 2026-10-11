import { act, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import type { LibraryStatistics } from '../../shared/types/generated/application/LibraryStatistics';

const mocks = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: any }) => void>(),
  invoke: vi.fn(),
  getConfig: vi.fn(),
}));

vi.mock('../../platform/ipc', () => ({
  invoke: mocks.invoke,
  listen: vi.fn(async (name: string, handler: (event: { payload: any }) => void) => {
    mocks.handlers.set(name, handler);
    return () => mocks.handlers.delete(name);
  }),
  normalizeInvokeError: (reason: unknown) => reason instanceof Error ? reason : new Error(String(reason)),
}));

vi.mock('../../shared/ui/ColorPicker', () => ({ ColorPicker: () => null }));
vi.mock('../../shared/ui/IconPicker', () => ({ IconPicker: () => null }));
vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));
vi.mock('../../shared/ui/ContextMenu', () => ({
  ContextMenu: () => null,
  useContextMenu: () => ({ state: null, openAt: vi.fn(), close: vi.fn() }),
}));
vi.mock('../../shared/ui/WindowControls', () => ({ WindowCloseButton: () => null }));
vi.mock('../subscriptions/components/SubscriptionCoverDialog', () => ({ MediaCoverDialog: () => null }));
vi.mock('./LibraryAvatar', () => ({ LibraryAvatar: () => null }));

import { LibraryManager } from './LibraryManager';

function statistics(tags: number): LibraryStatistics {
  return {
    active_items: 502,
    inbox_items: 0,
    trash_items: 0,
    standalone_items: 502,
    collections: 0,
    media_assets: 502,
    image_assets: 501,
    video_assets: 1,
    audio_assets: 0,
    other_assets: 0,
    physical_files: 502,
    original_bytes: 1_000,
    tags,
    folders: 1,
    smart_folders: 0,
    subscriptions: 0,
    revision: 1,
  };
}

describe('LibraryManager statistics', () => {
  beforeEach(() => {
    mocks.handlers.clear();
    mocks.invoke.mockReset();
    mocks.getConfig.mockReset();
  });

  it('shows statistics from the switched library and ignores the superseded response', async () => {
    let currentPath = '/Pictures/Pack target.library';
    let resolveOldStatistics: ((value: LibraryStatistics) => void) | undefined;
    const oldStatistics = new Promise<LibraryStatistics>((resolve) => { resolveOldStatistics = resolve; });
    let statisticsReads = 0;
    mocks.getConfig.mockImplementation(async () => ({
      currentPath,
      libraryHistory: ['/Pictures/Pack target.library', '/Pictures/Cloud restored.library'],
      pinnedLibraries: [],
      libraryMeta: {},
      existsMap: {
        '/Pictures/Pack target.library': true,
        '/Pictures/Cloud restored.library': true,
      },
    }));
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'library.stats') {
        statisticsReads += 1;
        return statisticsReads === 1 ? oldStatistics : statistics(1);
      }
      if (command === 'cloud.providers.detect') return [];
      if (command === 'cloud.configuration.get') return { root_path: null };
      if (command === 'cloud.status.get') return null;
      return null;
    });
    Object.defineProperty(window, 'picto', {
      configurable: true,
      value: {
        library: { getConfig: mocks.getConfig },
        api: { window: { call: vi.fn().mockResolvedValue(null) } },
        shell: { showInFolder: vi.fn().mockResolvedValue(null) },
      },
    });

    renderWithProviders(<LibraryManager />);
    await waitFor(() => expect(statisticsReads).toBe(1));
    await waitFor(() => expect(mocks.handlers.has('library-switched')).toBe(true));

    currentPath = '/Pictures/Cloud restored.library';
    act(() => mocks.handlers.get('library-switched')?.({ payload: { path: currentPath } }));

    expect(await screen.findAllByText('Cloud restored')).toHaveLength(2);
    await waitFor(() => expect(screen.getByText('Tags').parentElement).toHaveTextContent('1Tags'));

    await act(async () => { resolveOldStatistics?.(statistics(2)); });
    expect(screen.getByText('Tags').parentElement).toHaveTextContent('1Tags');
    expect(statisticsReads).toBe(2);
  });
});
