import { act, render, screen } from '@testing-library/react';
import { Provider, getDefaultStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { activeNodeIdAtom, displayedSurfaceNodeIdAtom } from '../../state/navigation';
import { gridChromeTransitionAtom } from '../../state/grid';
import { GridScreen } from './GridScreen';

vi.mock('../../controllers/gridController', () => ({
  gridController: {
    navigateTo: vi.fn(async () => {}),
    deactivate: vi.fn(),
    loadFirstPage: vi.fn(async () => {}),
    loadNextPage: vi.fn(async () => {}),
  },
}));
vi.mock('./hooks/useGridArrowNav', () => ({ useGridArrowNav: vi.fn() }));
vi.mock('../../shared/ui/ContextMenu', () => ({
  ContextMenu: () => null,
  useContextMenu: () => ({ state: null, openAt: vi.fn(), close: vi.fn() }),
}));
vi.mock('../managers/ManagerSurface', () => ({
  ManagerSurface: ({ nodeId }: { nodeId: string }) => <div>Manager: {nodeId}</div>,
}));
vi.mock('../../shared/ui/EmptyState', () => ({
  EmptyState: () => <div>Grid surface</div>,
  EmptyStateAction: ({ children }: { children: React.ReactNode }) => <button>{children}</button>,
}));
vi.mock('../../shared/ui/ApplicationMenuButton/ApplicationMenuButton', () => ({ ApplicationMenuButton: () => null }));
vi.mock('./canvas/CanvasGrid', () => ({ CanvasGrid: () => <div>Grid surface</div> }));
vi.mock('./SubfolderGrid', () => ({ SubfolderGrid: () => null }));
vi.mock('../viewer/MediaView', () => ({ MediaView: () => null }));
vi.mock('../viewer/QuickLook', () => ({ QuickLook: () => null }));
vi.mock('../tags/TagSelectPanel', () => ({ TagSelectPanel: () => null }));
vi.mock('../folders/FolderPickerPanel', () => ({ FolderPickerPanel: () => null }));
vi.mock('../ai-tagger/AiTaggerPanel', () => ({ AiTaggerPanel: () => null }));

describe('GridScreen universal surface timeline', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('keeps the outgoing grid or manager mounted until the common fade midpoint', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridChromeTransitionAtom, 'stable');

    render(
      <Provider store={store}>
        <MantineProvider><GridScreen /></MantineProvider>
      </Provider>,
    );

    await act(async () => { vi.advanceTimersByTime(400); });
    expect(screen.getByText('Grid surface')).toBeInTheDocument();

    await act(async () => { store.set(activeNodeIdAtom, 'system:subscriptions'); });
    expect(store.get(gridChromeTransitionAtom)).toBe('leaving_grid');
    const outgoingGrid = screen.getByText('Grid surface');
    expect(outgoingGrid).toBeInTheDocument();
    expect(outgoingGrid.parentElement?.className).toContain('surfaceFadeOut');

    await act(async () => { vi.advanceTimersByTime(170); });
    expect(store.get(displayedSurfaceNodeIdAtom)).toBe('system:subscriptions');
    expect(screen.getByText('Manager: system:subscriptions')).toBeInTheDocument();

    await act(async () => { vi.advanceTimersByTime(400); });
    await act(async () => { store.set(activeNodeIdAtom, 'system:tag_manager'); });
    expect(store.get(gridChromeTransitionAtom)).toBe('stable');
    expect(screen.getByText('Manager: system:subscriptions')).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(170); });
    expect(screen.getByText('Manager: system:tag_manager')).toBeInTheDocument();

    await act(async () => { vi.advanceTimersByTime(400); });
    await act(async () => { store.set(activeNodeIdAtom, 'system:duplicates'); });
    expect(screen.getByText('Manager: system:tag_manager')).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(170); });
    expect(screen.getByText('Manager: system:duplicates')).toBeInTheDocument();

    await act(async () => { vi.advanceTimersByTime(400); });
    await act(async () => { store.set(activeNodeIdAtom, 'system:active'); });
    expect(store.get(gridChromeTransitionAtom)).toBe('stable');
    expect(screen.getByText('Manager: system:duplicates')).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(170); });
    expect(store.get(gridChromeTransitionAtom)).toBe('entering_grid');
    expect(store.get(displayedSurfaceNodeIdAtom)).toBe('system:active');
    expect(screen.getByText('Grid surface')).toBeInTheDocument();

    await act(async () => { vi.advanceTimersByTime(400); });
    await act(async () => { store.set(activeNodeIdAtom, 'folder:1'); });
    expect(store.get(gridChromeTransitionAtom)).toBe('stable');
    expect(screen.getByText('Grid surface')).toBeInTheDocument();
  });
});
