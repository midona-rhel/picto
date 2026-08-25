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

  it('activates the enabled primary action when Enter is pressed in an input', () => {
    const onPrimary = vi.fn();
    render(
      <GlassModal
        open
        title="Rename"
        onClose={vi.fn()}
        footer={<button data-modal-primary="true" onClick={onPrimary}>Save</button>}
      >
        <input aria-label="Name" />
      </GlassModal>,
    );

    fireEvent.keyDown(screen.getByLabelText('Name'), { key: 'Enter' });

    expect(onPrimary).toHaveBeenCalledOnce();
  });

  it('does not replace the native Enter behavior of a focused button', () => {
    const onSecondary = vi.fn();
    const onPrimary = vi.fn();
    render(
      <GlassModal
        open
        title="Confirm"
        onClose={vi.fn()}
        footer={<button data-modal-primary="true" onClick={onPrimary}>Confirm</button>}
      >
        <button onClick={onSecondary}>Alternative</button>
      </GlassModal>,
    );

    fireEvent.keyDown(screen.getByRole('button', { name: 'Alternative' }), { key: 'Enter' });

    expect(onPrimary).not.toHaveBeenCalled();
    expect(onSecondary).not.toHaveBeenCalled();
  });
});
