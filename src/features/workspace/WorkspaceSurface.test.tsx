import { act, render, screen } from '@testing-library/react';
import { Provider, getDefaultStore } from 'jotai';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { activeNodeIdAtom, displayedSurfaceNodeIdAtom } from '../../state/navigation';
import { gridDrilldownAtom, gridSessionAtom, gridTransitionPhaseAtom, pendingGridIntentAtom, pendingGridNavigationAtom } from '../../state/grid';
import { viewerExitTransitionAtom, viewerSessionAtom } from '../../state/viewer';
import { gridController } from '../../controllers/gridController';
import { WorkspaceSurface } from './WorkspaceSurface';

vi.mock('../../controllers/gridController', () => ({
  gridController: {
    navigateTo: vi.fn(async () => {}),
    prepareNavigation: vi.fn(async () => ({ scopeKey: 'test', session: {} })),
    commitNavigation: vi.fn(),
    deactivate: vi.fn(),
    applyIntent: vi.fn(),
  },
}));
vi.mock('../grid/GridScreen', () => ({
  GridScreen: ({ nodeId, initialScrollPosition, onFirstPaint }: {
    nodeId: string;
    initialScrollPosition?: { scrollTop: number; progress: number } | null;
    onFirstPaint?: () => void;
  }) => (
    <div data-testid="grid-screen" data-scroll-top={initialScrollPosition?.scrollTop ?? ''}>
      Grid: {nodeId}
      <button type="button" onClick={onFirstPaint}>Commit frame</button>
    </div>
  ),
}));
vi.mock('../managers/ManagerSurface', () => ({
  ManagerSurface: ({ nodeId }: { nodeId: string }) => <div>Manager: {nodeId}</div>,
}));

describe('workspace surface coordinator', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(gridController.navigateTo).mockClear();
    vi.mocked(gridController.prepareNavigation).mockReset();
    vi.mocked(gridController.prepareNavigation).mockResolvedValue({ scopeKey: 'test', session: {} as never });
    vi.mocked(gridController.commitNavigation).mockClear();
  });
  afterEach(() => { vi.useRealTimers(); });

  it('loads the default All scope when the workspace first mounts', () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');

    render(<Provider store={store}><WorkspaceSurface /></Provider>);

    expect(gridController.navigateTo).toHaveBeenCalledOnce();
    expect(gridController.navigateTo).toHaveBeenCalledWith({ kind: 'all' });
  });

  it('keeps the outgoing surface mounted until the single midpoint commit', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    render(<Provider store={store}><WorkspaceSurface /></Provider>);
    expect(screen.getByText('Grid: system:active')).toBeInTheDocument();

    await act(async () => store.set(activeNodeIdAtom, 'system:subscriptions'));
    expect(store.get(gridTransitionPhaseAtom)).toBe('fading_out');
    expect(screen.getByText('Grid: system:active')).toBeInTheDocument();

    await act(async () => vi.advanceTimersByTime(170));
    expect(store.get(displayedSurfaceNodeIdAtom)).toBe('system:subscriptions');
    expect(screen.getByText('Manager: system:subscriptions')).toBeInTheDocument();

    await act(async () => vi.advanceTimersByTime(200));
    await act(async () => store.set(activeNodeIdAtom, 'folder:7'));
    expect(screen.getByText('Manager: system:subscriptions')).toBeInTheDocument();
    await act(async () => vi.advanceTimersByTime(170));
    expect(screen.getByText('Grid: folder:7')).toBeInTheDocument();
  });

  it('prefetches the destination while the outgoing grid fades', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    render(<Provider store={store}><WorkspaceSurface /></Provider>);
    vi.mocked(gridController.prepareNavigation).mockClear();

    await act(async () => store.set(activeNodeIdAtom, 'folder:7'));

    expect(store.get(gridTransitionPhaseAtom)).toBe('fading_out');
    expect(screen.getByText('Grid: system:active')).toBeInTheDocument();
    expect(gridController.prepareNavigation).toHaveBeenCalledWith({ kind: 'folder', folder_id: 7 });

    await act(async () => vi.advanceTimersByTime(170));

    expect(screen.getByText('Grid: folder:7')).toBeInTheDocument();
  });

  it('restores the hidden grid before revealing the replacement', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    render(<Provider store={store}><WorkspaceSurface /></Provider>);

    await act(async () => store.set(activeNodeIdAtom, 'folder:7'));
    await act(async () => vi.advanceTimersByTime(170));
    expect(store.get(gridTransitionPhaseAtom)).toBe('waiting');
    expect(screen.getByTestId('grid-screen')).toHaveAttribute('data-scroll-top', '0');

    await act(async () => vi.advanceTimersByTime(16));
    expect(store.get(gridTransitionPhaseAtom)).toBe('waiting');

    screen.getByRole('button', { name: 'Commit frame' }).click();
    await act(async () => vi.advanceTimersByTime(16));
    expect(store.get(gridTransitionPhaseAtom)).toBe('fading_in');
  });

  it('does not let the outgoing grid consume the destination paint signal', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    let finishLoad: (() => void) | undefined;
    vi.mocked(gridController.prepareNavigation).mockImplementation(
      () => new Promise((resolve) => { finishLoad = () => resolve({ scopeKey: 'folder:7', session: {} as never }); }),
    );
    render(<Provider store={store}><WorkspaceSurface /></Provider>);

    await act(async () => store.set(activeNodeIdAtom, 'folder:7'));
    await act(async () => vi.advanceTimersByTime(170));

    expect(store.get(gridTransitionPhaseAtom)).toBe('waiting');
    expect(screen.getByText('Grid: system:active')).toBeInTheDocument();
    expect(screen.getByTestId('grid-screen')).toHaveAttribute('data-scroll-top', '');
    screen.getByRole('button', { name: 'Commit frame' }).click();
    await act(async () => vi.advanceTimersByTime(16));
    expect(store.get(gridTransitionPhaseAtom)).toBe('waiting');

    await act(async () => { finishLoad?.(); });
    expect(screen.getByText('Grid: folder:7')).toBeInTheDocument();
    expect(screen.getByTestId('grid-screen')).toHaveAttribute('data-scroll-top', '0');
    screen.getByRole('button', { name: 'Commit frame' }).click();
    await act(async () => vi.advanceTimersByTime(16));
    expect(store.get(gridTransitionPhaseAtom)).toBe('fading_in');
  });

  it('commits the latest destination at the original midpoint when navigation is clicked repeatedly', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    render(<Provider store={store}><WorkspaceSurface /></Provider>);
    vi.mocked(gridController.prepareNavigation).mockClear();

    await act(async () => store.set(activeNodeIdAtom, 'folder:7'));
    await act(async () => vi.advanceTimersByTime(65));
    await act(async () => store.set(activeNodeIdAtom, 'folder:9'));
    await act(async () => vi.advanceTimersByTime(105));

    expect(store.get(displayedSurfaceNodeIdAtom)).toBe('folder:9');
    expect(gridController.prepareNavigation).toHaveBeenCalledTimes(2);
    expect(gridController.prepareNavigation).toHaveBeenLastCalledWith({ kind: 'folder', folder_id: 9 });
  });

  it('does not fade in until the destination load completes', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    render(<Provider store={store}><WorkspaceSurface /></Provider>);

    let finishLoad: (() => void) | undefined;
    vi.mocked(gridController.prepareNavigation).mockClear();
    vi.mocked(gridController.prepareNavigation).mockImplementation(
      () => new Promise((resolve) => { finishLoad = () => resolve({ scopeKey: 'folder:7', session: {} as never }); }),
    );

    await act(async () => store.set(activeNodeIdAtom, 'folder:7'));
    await act(async () => vi.advanceTimersByTime(170));
    await act(async () => vi.advanceTimersByTime(32));
    expect(store.get(gridTransitionPhaseAtom)).toBe('waiting');
    expect(gridController.commitNavigation).not.toHaveBeenCalled();

    await act(async () => { finishLoad?.(); });
    expect(gridController.commitNavigation).toHaveBeenCalledOnce();
    expect(store.get(gridTransitionPhaseAtom)).toBe('waiting');
    screen.getByRole('button', { name: 'Commit frame' }).click();
    await act(async () => vi.advanceTimersByTime(16));
    expect(store.get(gridTransitionPhaseAtom)).toBe('fading_in');
  });

  it('redirects immediately when the hidden replacement is still loading', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    render(<Provider store={store}><WorkspaceSurface /></Provider>);
    vi.mocked(gridController.prepareNavigation).mockClear();

    await act(async () => store.set(activeNodeIdAtom, 'folder:7'));
    await act(async () => vi.advanceTimersByTime(170));
    await act(async () => {
      store.set(gridSessionAtom, { ...store.get(gridSessionAtom), status: 'loading' });
      store.set(activeNodeIdAtom, 'folder:9');
    });

    expect(store.get(displayedSurfaceNodeIdAtom)).toBe('folder:9');
    expect(gridController.prepareNavigation).toHaveBeenLastCalledWith({ kind: 'folder', folder_id: 9 });
  });

  it('keeps detail view state through fade-out and closes it at the midpoint', async () => {
    const store = getDefaultStore();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    store.set(viewerSessionAtom, { currentIndex: 0, currentItemId: 7 });
    store.set(viewerExitTransitionAtom, true);

    render(<Provider store={store}><WorkspaceSurface /></Provider>);

    await act(async () => store.set(activeNodeIdAtom, 'system:inbox'));
    expect(store.get(gridTransitionPhaseAtom)).toBe('fading_out');
    expect(store.get(viewerSessionAtom)).not.toBeNull();

    await act(async () => vi.advanceTimersByTime(170));
    expect(store.get(viewerSessionAtom)).toBeNull();
    expect(store.get(displayedSurfaceNodeIdAtom)).toBe('system:inbox');
  });

  it('transitions a manager-owned filtered grid at the normal surface midpoint', async () => {
    const store = getDefaultStore();
    const filters = {
      ...store.get(gridSessionAtom).filters,
      include_tags: [{ tag_id: 1, name: 'character:alice' }],
    };
    store.set(activeNodeIdAtom, 'system:tag_manager');
    store.set(displayedSurfaceNodeIdAtom, 'system:tag_manager');
    store.set(gridDrilldownAtom, null);
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: false, status: 'idle' });

    render(<Provider store={store}><WorkspaceSurface /></Provider>);
    expect(screen.getByText('Manager: system:tag_manager')).toBeInTheDocument();

    await act(async () => {
      store.set(pendingGridNavigationAtom, { nodeId: 'system:active', filters });
      store.set(gridDrilldownAtom, {
        ownerNodeId: 'system:tag_manager',
        scopeNodeId: 'system:active',
        filters,
      });
    });

    expect(store.get(gridTransitionPhaseAtom)).toBe('fading_out');
    expect(screen.getByText('Manager: system:tag_manager')).toBeInTheDocument();

    await act(async () => vi.advanceTimersByTime(170));

    expect(screen.getByText('Grid: system:active')).toBeInTheDocument();
    expect(gridController.prepareNavigation).toHaveBeenLastCalledWith(
      { kind: 'all' },
      { filters },
    );
  });

  it('applies filters in place without starting a navigation fade', async () => {
    const store = getDefaultStore();
    const filters = {
      ...store.get(gridSessionAtom).filters,
      color_hex: '#FF2727',
    };
    store.set(activeNodeIdAtom, 'system:active');
    store.set(displayedSurfaceNodeIdAtom, 'system:active');
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), active: true, status: 'idle' });
    store.set(gridTransitionPhaseAtom, 'idle');
    store.set(pendingGridIntentAtom, null);

    render(<Provider store={store}><WorkspaceSurface /></Provider>);
    vi.mocked(gridController.applyIntent).mockClear();

    await act(async () => store.set(pendingGridIntentAtom, { type: 'filter', filters }));

    expect(store.get(gridTransitionPhaseAtom)).toBe('idle');
    expect(gridController.applyIntent).toHaveBeenCalledWith({ type: 'filter', filters });
  });
});
