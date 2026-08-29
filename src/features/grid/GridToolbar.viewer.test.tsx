import { act, fireEvent, render, screen } from '@testing-library/react';
import { createStore, Provider } from 'jotai';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { viewerDisplayControlsAtom, viewerDisplayStateAtom } from '../../state/viewer';
import { ViewerToolbar } from './GridToolbar';

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

describe('ViewerToolbar live zoom display', () => {
  it('updates the label and slider from live frames without publishing React state', () => {
    const store = createStore();
    let liveListener: ((scale: number) => void) | null = null;
    store.set(viewerDisplayStateAtom, { currentIndex: 0, total: 3, zoomPercent: 100 });
    store.set(viewerDisplayControlsAtom, {
      close: vi.fn(),
      navigate: vi.fn(),
      zoom: {
        fitToWindow: vi.fn(),
        fitActual: vi.fn(),
        zoomIn: vi.fn(),
        zoomOut: vi.fn(),
        setZoomScale: vi.fn(),
        subscribeZoomScale: (listener) => {
          liveListener = listener;
          listener(1);
          return vi.fn();
        },
      },
    });

    render(<Provider store={store}><ViewerToolbar /></Provider>);
    const slider = screen.getByRole('slider', { name: 'Zoom' }) as HTMLInputElement;
    expect(screen.getByText('100%')).toBeInTheDocument();
    expect(Number(slider.value)).toBeCloseTo(50);

    act(() => liveListener?.(0.25));

    expect(screen.getByText('25%')).toBeInTheDocument();
    expect(Number(slider.value)).toBeLessThan(50);
    expect(store.get(viewerDisplayStateAtom)?.zoomPercent).toBe(100);
  });

  it('uses the same detail toolbar for groups without media-only controls', () => {
    const store = createStore();
    const navigate = vi.fn();
    const edit = vi.fn();
    store.set(viewerDisplayStateAtom, { currentIndex: 1, total: 3 });
    store.set(viewerDisplayControlsAtom, {
      close: vi.fn(),
      navigate,
      edit,
    });

    render(<Provider store={store}><ViewerToolbar /></Provider>);
    expect(screen.getByText('2 / 3')).toBeInTheDocument();
    expect(screen.queryByRole('slider', { name: 'Zoom' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Fit to window' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Actual size' })).not.toBeInTheDocument();
    const editButton = screen.getByRole('button', { name: 'Edit group' });
    expect(editButton.querySelector('[data-picto-icon="group-edit"]')).toBeInTheDocument();
    editButton.click();
    screen.getByRole('button', { name: 'Previous' }).click();
    screen.getByRole('button', { name: 'Next' }).click();
    expect(edit).toHaveBeenCalledOnce();
    expect(navigate.mock.calls).toEqual([[-1], [1]]);
  });

  it('keeps the originating scope breadcrumb visible while editing a collection', () => {
    const store = createStore();
    const close = vi.fn();
    store.set(viewerDisplayStateAtom, {
      currentIndex: 0,
      total: 1,
      breadcrumb: { parent: 'All', current: 'Reference set' },
    });
    store.set(viewerDisplayControlsAtom, { close, backLabel: 'Back to grid' });

    render(<Provider store={store}><ViewerToolbar /></Provider>);
    expect(screen.getByLabelText('All / Reference set')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'All' }));
    expect(close).toHaveBeenCalledOnce();
  });

  it('exposes standard zoom levels and toggles actual/fit on right click', () => {
    const store = createStore();
    const fitToWindow = vi.fn();
    const setZoomScale = vi.fn();
    store.set(viewerDisplayStateAtom, { currentIndex: 0, total: 1, zoomPercent: 100 });
    store.set(viewerDisplayControlsAtom, {
      close: vi.fn(),
      zoom: {
        fitToWindow,
        fitActual: vi.fn(),
        zoomIn: vi.fn(),
        zoomOut: vi.fn(),
        setZoomScale,
        subscribeZoomScale: () => vi.fn(),
      },
    });

    render(<Provider store={store}><ViewerToolbar /></Provider>);
    const zoomLabel = screen.getByRole('button', { name: '100%' });
    fireEvent.click(zoomLabel, { clientX: 20, clientY: 20 });
    fireEvent.click(screen.getByRole('menuitem', { name: '25%' }));
    expect(setZoomScale).toHaveBeenCalledWith(0.25);

    fireEvent.contextMenu(zoomLabel);
    expect(fitToWindow).toHaveBeenCalledOnce();
  });
});
