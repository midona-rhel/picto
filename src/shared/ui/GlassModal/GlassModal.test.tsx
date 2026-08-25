import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { GlassModal } from './GlassModal';

describe('GlassModal outside dismissal', () => {
  afterEach(() => { vi.useRealTimers(); });

  it('does not close when a selection drag starts inside and releases outside', () => {
    vi.useFakeTimers();
    const onClose = vi.fn();
    render(<GlassModal open title="Rename" onClose={onClose}>
      <input aria-label="Name" defaultValue="Selected text" />
    </GlassModal>);
    const input = screen.getByLabelText('Name');
    const backdrop = screen.getByRole('dialog').parentElement!;

    fireEvent.pointerDown(input);
    fireEvent.pointerUp(backdrop);
    act(() => vi.advanceTimersByTime(200));

    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('closes when the press begins and ends on the backdrop', () => {
    vi.useFakeTimers();
    const onClose = vi.fn();
    render(<GlassModal open title="Rename" onClose={onClose}><div>Body</div></GlassModal>);
    const backdrop = screen.getByRole('dialog').parentElement!;

    fireEvent.pointerDown(backdrop);
    fireEvent.pointerUp(backdrop);
    act(() => vi.advanceTimersByTime(120));

    expect(onClose).toHaveBeenCalledOnce();
  });
});
