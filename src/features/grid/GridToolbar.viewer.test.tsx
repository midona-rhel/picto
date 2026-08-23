import { act, render, screen } from '@testing-library/react';
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
});
