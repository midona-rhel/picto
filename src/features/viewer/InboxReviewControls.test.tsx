import { act, fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resetShortcutRuntimeForTests } from '../../runtime/shortcutRuntime';
import { InboxReviewControls, resolveInboxReviewItemId } from './InboxReviewControls';

const { showErrorNotification } = vi.hoisted(() => ({ showErrorNotification: vi.fn() }));
vi.mock('../../shared/lib/notifications', () => ({ showErrorNotification }));
vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children, label, shortcutId }: { children: ReactNode; label: string; shortcutId?: string }) => (
    <span data-tooltip-label={label} data-tooltip-shortcut-id={shortcutId ?? ''}>{children}</span>
  ),
}));

describe('InboxReviewControls', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetShortcutRuntimeForTests();
    showErrorNotification.mockReset();
  });

  afterEach(() => {
    resetShortcutRuntimeForTests();
    vi.useRealTimers();
  });

  it('shows review controls for the viewed inbox entity, including groups', () => {
    expect(resolveInboxReviewItemId(
      { item_id: 42, lifecycle: 'inbox' },
      null,
    )).toBe(42);
    expect(resolveInboxReviewItemId(
      null,
      { item_id: 99, lifecycle: 'inbox' },
    )).toBe(99);
    expect(resolveInboxReviewItemId(
      { item_id: 42, lifecycle: 'active' },
      null,
    )).toBeNull();
  });

  it('accepts with Z and advances only after persistence and the exit animation', async () => {
    let resolveCommit: (() => void) | null = null;
    const onCommit = vi.fn(() => new Promise<void>((resolve) => { resolveCommit = resolve; }));
    const onAdvance = vi.fn();
    render(<InboxReviewControls itemId={42} onCommit={onCommit} onAdvance={onAdvance} />);

    expect(document.querySelector('[data-tooltip-label="Accept"]')).toHaveAttribute('data-tooltip-shortcut-id', 'inbox.accept');
    expect(document.querySelector('[data-tooltip-label="Reject"]')).toHaveAttribute('data-tooltip-shortcut-id', 'inbox.reject');
    expect(screen.getByRole('button', { name: 'Accept item' })).not.toHaveAttribute('title');

    fireEvent.keyDown(window, { key: 'z' });

    expect(onCommit).toHaveBeenCalledWith(42, 'accept');
    expect(document.querySelector('[data-inbox-review-controls]')).toHaveAttribute('data-review-decision', 'accept');
    expect(screen.getByRole('button', { name: 'Accept item' })).toBeDisabled();
    expect(onAdvance).not.toHaveBeenCalled();

    await act(async () => { resolveCommit?.(); });
    expect(onAdvance).not.toHaveBeenCalled();
    await act(async () => { vi.runAllTimers(); });
    expect(onAdvance).toHaveBeenCalledOnce();
  });

  it('rejects with X and reports failures without advancing', async () => {
    const onCommit = vi.fn().mockRejectedValue(new Error('write failed'));
    const onAdvance = vi.fn();
    render(<InboxReviewControls itemId={9} onCommit={onCommit} onAdvance={onAdvance} />);

    fireEvent.keyDown(window, { key: 'x' });
    await act(async () => {});

    expect(onCommit).toHaveBeenCalledWith(9, 'reject');
    expect(showErrorNotification).toHaveBeenCalledWith({
      title: 'Unable to reject item',
      message: 'write failed',
    });
    expect(onAdvance).not.toHaveBeenCalled();
    expect(document.querySelector('[data-inbox-review-controls]')).toHaveAttribute('data-review-decision', 'idle');
  });
});
