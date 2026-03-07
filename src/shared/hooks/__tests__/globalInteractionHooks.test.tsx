import { render } from '@testing-library/react';
import { describe, expect, it, vi, afterEach } from 'vitest';
import { useGlobalKeydown } from '../useGlobalKeydown';
import { useGlobalPointerDrag } from '../useGlobalPointerDrag';

function KeydownHarness({ enabled = true }: { enabled?: boolean }) {
  useGlobalKeydown(() => {}, enabled);
  return null;
}

function PointerDragHarness({
  active,
  target,
}: {
  active: boolean;
  target?: 'window' | 'document';
}) {
  useGlobalPointerDrag({ onMove: () => {}, onEnd: () => {} }, active, { target });
  return null;
}

describe('global interaction hooks', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('attaches and detaches document keydown listener deterministically', () => {
    const addSpy = vi.spyOn(document, 'addEventListener');
    const removeSpy = vi.spyOn(document, 'removeEventListener');

    const { rerender, unmount } = render(<KeydownHarness enabled />);
    expect(addSpy).toHaveBeenCalledWith('keydown', expect.any(Function), false);

    rerender(<KeydownHarness enabled={false} />);
    expect(removeSpy).toHaveBeenCalledWith('keydown', expect.any(Function), false);

    unmount();
  });

  it('attaches and detaches window drag listeners only while active', () => {
    const addSpy = vi.spyOn(window, 'addEventListener');
    const removeSpy = vi.spyOn(window, 'removeEventListener');

    const { rerender, unmount } = render(<PointerDragHarness active={false} />);
    expect(addSpy).not.toHaveBeenCalledWith('mousemove', expect.any(Function));

    rerender(<PointerDragHarness active />);
    expect(addSpy).toHaveBeenCalledWith('mousemove', expect.any(Function));
    expect(addSpy).toHaveBeenCalledWith('mouseup', expect.any(Function));

    rerender(<PointerDragHarness active={false} />);
    expect(removeSpy).toHaveBeenCalledWith('mousemove', expect.any(Function));
    expect(removeSpy).toHaveBeenCalledWith('mouseup', expect.any(Function));

    unmount();
  });

  it('can target document for drag listeners', () => {
    const addSpy = vi.spyOn(document, 'addEventListener');
    const removeSpy = vi.spyOn(document, 'removeEventListener');

    const { unmount } = render(<PointerDragHarness active target="document" />);
    expect(addSpy).toHaveBeenCalledWith('mousemove', expect.any(Function));
    expect(addSpy).toHaveBeenCalledWith('mouseup', expect.any(Function));

    unmount();
    expect(removeSpy).toHaveBeenCalledWith('mousemove', expect.any(Function));
    expect(removeSpy).toHaveBeenCalledWith('mouseup', expect.any(Function));
  });
});
