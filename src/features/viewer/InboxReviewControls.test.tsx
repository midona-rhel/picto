import { act, fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resetShortcutRuntimeForTests } from '../../runtime/shortcutRuntime';
import { InboxReviewControls, resolveInboxReviewItemId } from './InboxReviewControls';

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children, label, shortcutId }: { children: ReactNode; label: string; shortcutId?: string }) => (
    <span data-tooltip-label={label} data-tooltip-shortcut-id={shortcutId ?? ''}>{children}</span>
  ),
}));

describe('InboxReviewControls', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetShortcutRuntimeForTests();
  });

  afterEach(() => {
    resetShortcutRuntimeForTests();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('shows review controls for the viewed inbox entity, including groups', () => {
    expect(resolveInboxReviewItemId(
      { root_id: 42, lifecycle: 'inbox' },
      null,
    )).toBe(42);
    expect(resolveInboxReviewItemId(
      null,
      { root_id: 99, lifecycle: 'inbox' },
    )).toBe(99);
    expect(resolveInboxReviewItemId(
      { root_id: 42, lifecycle: 'active' },
      null,
    )).toBeNull();
  });

  it('reviews with Z and X at once, without waiting for the save', () => {
    const onReview = vi.fn(() => true);
    render(<InboxReviewControls itemId={42} onReview={onReview} />);

    expect(document.querySelector('[data-tooltip-label="Accept"]')).toHaveAttribute('data-tooltip-shortcut-id', 'inbox.accept');
    expect(document.querySelector('[data-tooltip-label="Reject"]')).toHaveAttribute('data-tooltip-shortcut-id', 'inbox.reject');
    expect(screen.getByRole('button', { name: 'Accept item' })).not.toHaveAttribute('title');

    fireEvent.keyDown(window, { key: 'z' });
    expect(onReview).toHaveBeenCalledWith(42, 'accept');
    expect(document.querySelector('[data-inbox-review-controls]')).toHaveAttribute('data-review-decision', 'accept');
  });

  it.each(['Accept', 'Reject'])('flashes %s and stays on screen while the next item takes over', (action) => {
    const onReview = vi.fn(() => true);
    const { rerender } = render(<InboxReviewControls itemId={42} onReview={onReview} />);
    const controls = document.querySelector('[data-inbox-review-controls]');

    fireEvent.click(screen.getByRole('button', { name: `${action} item` }));
    expect(onReview).toHaveBeenCalledWith(42, action.toLowerCase());
    rerender(<InboxReviewControls itemId={43} onReview={onReview} />);

    // The same controls, not faded out and back in.
    expect(document.querySelector('[data-inbox-review-controls]')).toBe(controls);
    expect(controls).toHaveAttribute('data-review-decision', action.toLowerCase());
    act(() => { vi.advanceTimersByTime(360); });
    expect(controls).toHaveAttribute('data-review-decision', 'idle');
    expect(screen.getByRole('button', { name: `${action} item` })).toBeEnabled();
  });

  it('flashes only when the owner takes the review', () => {
    // The owner refuses a second review of an item it is still saving.
    const taken = new Set<number>();
    const onReview = vi.fn((itemId: number) => {
      if (taken.has(itemId)) return false;
      taken.add(itemId);
      return true;
    });
    render(<InboxReviewControls itemId={42} onReview={onReview} />);
    const controls = document.querySelector('[data-inbox-review-controls]');

    fireEvent.keyDown(window, { key: 'z' });
    expect(controls).toHaveAttribute('data-review-decision', 'accept');
    act(() => { vi.advanceTimersByTime(360); });
    fireEvent.keyDown(window, { key: 'x' });
    expect(onReview).toHaveBeenLastCalledWith(42, 'reject');
    expect(controls).toHaveAttribute('data-review-decision', 'idle');

    // Once the owner takes reviews of it again (its save failed), so do the controls.
    taken.delete(42);
    fireEvent.keyDown(window, { key: 'x' });
    expect(controls).toHaveAttribute('data-review-decision', 'reject');
  });

  it('takes no input while a review waits on a question, then resumes', () => {
    const onReview = vi.fn(() => true);
    const { rerender } = render(<InboxReviewControls itemId={43} onReview={onReview} paused />);

    fireEvent.keyDown(window, { key: 'x' });
    fireEvent.keyDown(window, { key: 'z' });
    expect(screen.getByRole('button', { name: 'Reject item' })).toBeDisabled();
    expect(onReview).not.toHaveBeenCalled();

    rerender(<InboxReviewControls itemId={43} onReview={onReview} />);
    fireEvent.keyDown(window, { key: 'z' });
    expect(onReview).toHaveBeenCalledWith(43, 'accept');
  });

  it('ignores repeated and modified key presses', () => {
    const onReview = vi.fn(() => true);
    render(<InboxReviewControls itemId={42} onReview={onReview} />);
    fireEvent.keyDown(window, { key: 'z', repeat: true });
    fireEvent.keyDown(window, { key: 'z', metaKey: true });
    expect(onReview).not.toHaveBeenCalled();
  });
});
