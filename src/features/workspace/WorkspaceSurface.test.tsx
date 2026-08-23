import { act, render, screen } from '@testing-library/react';
import { Provider, getDefaultStore } from 'jotai';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { activeNodeIdAtom, displayedSurfaceNodeIdAtom } from '../../state/navigation';
import { gridSessionAtom, gridTransitionPhaseAtom } from '../../state/grid';
import { WorkspaceSurface } from './WorkspaceSurface';

vi.mock('../../controllers/gridController', () => ({
  gridController: {
    navigateTo: vi.fn(async () => {}),
    deactivate: vi.fn(),
    applyIntent: vi.fn(),
  },
}));
vi.mock('../grid/GridScreen', () => ({
  GridScreen: ({ nodeId }: { nodeId: string }) => <div>Grid: {nodeId}</div>,
}));
vi.mock('../managers/ManagerSurface', () => ({
  ManagerSurface: ({ nodeId }: { nodeId: string }) => <div>Manager: {nodeId}</div>,
}));

describe('workspace surface coordinator', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

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
});
