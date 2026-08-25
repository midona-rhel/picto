import { fireEvent, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useRef } from 'react';
import { useImageZoom } from './useImageZoom';

class ResizeObserverStub {
  observe() {}
  disconnect() {}
}

function Harness({ trackpad }: { trackpad: boolean }) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);
  useImageZoom(
    viewportRef,
    { width: 1000, height: 1000 },
    [frameRef],
    { macTrackpadGestures: trackpad },
  );
  return <div ref={viewportRef} data-testid="viewport"><div ref={frameRef} data-testid="frame" /></div>;
}

describe('useImageZoom macOS trackpad gestures', () => {
  const originalPlatform = navigator.platform;

  beforeEach(() => {
    Object.defineProperty(navigator, 'platform', { value: 'MacIntel', configurable: true });
    vi.stubGlobal('ResizeObserver', ResizeObserverStub);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (id: number) => window.clearTimeout(id));
  });

  afterEach(() => {
    Object.defineProperty(navigator, 'platform', { value: originalPlatform, configurable: true });
    vi.unstubAllGlobals();
  });

  it('pans on ordinary two-finger wheel deltas', async () => {
    render(<Harness trackpad />);

    fireEvent.wheel(document.querySelector('[data-testid="viewport"]')!, {
      deltaX: 12,
      deltaY: 20,
      ctrlKey: false,
    });

    await waitFor(() => {
      const transform = document.querySelector<HTMLElement>('[data-testid="frame"]')!.style.transform;
      expect(transform).toContain('-12px');
      expect(transform).toContain('-20px');
      expect(transform).toContain('scale(1)');
    });
  });

  it('zooms rather than pans for pinch-marked wheel events', async () => {
    render(<Harness trackpad />);

    fireEvent.wheel(document.querySelector('[data-testid="viewport"]')!, {
      deltaX: 0,
      deltaY: -20,
      ctrlKey: true,
    });

    await waitFor(() => {
      expect(document.querySelector<HTMLElement>('[data-testid="frame"]')!.style.transform)
        .not.toContain('scale(1)');
    });
  });
});
