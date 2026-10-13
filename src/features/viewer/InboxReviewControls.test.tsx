import { act, fireEvent, render, screen } from '@testing-library/react';
import { useState, type ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resetShortcutRuntimeForTests } from '../../runtime/shortcutRuntime';
import { InboxReviewControls, resolveInboxReviewItemId } from './InboxReviewControls';
import styles from './InboxReviewControls.module.css';

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

  it('accepts with Z and advances only after persistence and the exit animation', async () => {
    let resolveCommit: (() => void) | null = null;
    const onCommit = vi.fn(() => new Promise<boolean>((resolve) => { resolveCommit = () => resolve(true); }));
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
    await act(async () => { await vi.runAllTimersAsync(); });
    expect(onAdvance).toHaveBeenCalledOnce();
  });

  it.each(['Accept', 'Reject'])('shows the full %s pulse before fading and advancing', async (action) => {
    const onCommit = vi.fn().mockResolvedValue(true);
    const onAdvance = vi.fn();
    render(<InboxReviewControls itemId={42} onCommit={onCommit} onAdvance={onAdvance} />);
    const controls = document.querySelector('[data-inbox-review-controls]');

    fireEvent.click(screen.getByRole('button', { name: `${action} item` }));
    await act(async () => {});
    expect(onCommit).toHaveBeenCalledWith(42, action.toLowerCase());
    expect(controls).not.toHaveClass(styles.leaving);
    expect(onAdvance).not.toHaveBeenCalled();

    await act(async () => { await vi.advanceTimersByTimeAsync(359); });
    expect(controls).not.toHaveClass(styles.leaving);
    expect(onAdvance).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(controls).toHaveClass(styles.leaving);
    expect(onAdvance).not.toHaveBeenCalled();

    await act(async () => { await vi.advanceTimersByTimeAsync(119); });
    expect(onAdvance).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(onAdvance).toHaveBeenCalledOnce();
  });

  it('waits for a slow commit without delaying again for a pulse that already finished', async () => {
    let resolveCommit!: () => void;
    const onCommit = vi.fn(() => new Promise<boolean>((resolve) => { resolveCommit = () => resolve(true); }));
    const onAdvance = vi.fn();
    render(<InboxReviewControls itemId={42} onCommit={onCommit} onAdvance={onAdvance} />);
    const controls = document.querySelector('[data-inbox-review-controls]');

    fireEvent.keyDown(window, { key: 'z' });
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    expect(controls).not.toHaveClass(styles.leaving);
    expect(onAdvance).not.toHaveBeenCalled();

    await act(async () => { resolveCommit(); });
    expect(controls).toHaveClass(styles.leaving);
    await act(async () => { await vi.advanceTimersByTimeAsync(119); });
    expect(onAdvance).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(onAdvance).toHaveBeenCalledOnce();
  });

  it('skips animation waits for reduced motion but still waits for persistence', async () => {
    const matchMedia = window.matchMedia.bind(window);
    vi.spyOn(window, 'matchMedia').mockImplementation((query) => ({
      ...matchMedia(query), matches: query === '(prefers-reduced-motion: reduce)',
    }));
    let resolveCommit!: () => void;
    const onCommit = vi.fn(() => new Promise<boolean>((resolve) => { resolveCommit = () => resolve(true); }));
    const onAdvance = vi.fn();
    render(<InboxReviewControls itemId={42} onCommit={onCommit} onAdvance={onAdvance} />);

    fireEvent.keyDown(window, { key: 'z' });
    await act(async () => {});
    expect(onAdvance).not.toHaveBeenCalled();
    await act(async () => { resolveCommit(); });
    // No timer advancement: with no animation, persistence is the only gate.
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
    expect(screen.getByRole('button', { name: 'Reject item' })).toBeEnabled();
    await act(async () => { await vi.runAllTimersAsync(); });
    expect(onAdvance).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: 'z' });
    await act(async () => {});
    expect(onCommit).toHaveBeenCalledTimes(2);
    expect(onCommit).toHaveBeenLastCalledWith(9, 'accept');
  });

  it('retargets the persistent controls without replaying their mount transition', () => {
    const onCommit = vi.fn(() => new Promise<boolean>(() => {}));
    const onAdvance = vi.fn();
    const { rerender } = render(
      <InboxReviewControls itemId={42} onCommit={onCommit} onAdvance={onAdvance} />,
    );
    const controls = document.querySelector('[data-inbox-review-controls]');

    rerender(<InboxReviewControls itemId={43} onCommit={onCommit} onAdvance={onAdvance} />);

    expect(document.querySelector('[data-inbox-review-controls]')).toBe(controls);
    fireEvent.click(screen.getByRole('button', { name: 'Accept item' }));
    expect(onCommit).toHaveBeenCalledWith(43, 'accept');
  });

  it.each(['z', 'x'])('keeps reconciliation from retargeting a %s review during its pulse and exit', async (key) => {
    const commits = vi.fn();
    const onAdvance = vi.fn();
    function ReconciledInbox() {
      const [items, setItems] = useState([42, 43, 44]);
      return <InboxReviewControls itemId={items[0]} onAdvance={onAdvance} onCommit={async (id, decision) => {
        commits(id, decision);
        setItems((current) => current.filter((item) => item !== id));
        return true;
      }} />;
    }
    render(<ReconciledInbox />);
    fireEvent.keyDown(window, { key });
    await act(async () => {});

    // The lifecycle delta already removed 42, but its review is still playing.
    fireEvent.keyDown(window, { key: 'z' });
    fireEvent.keyDown(window, { key: 'x' });
    fireEvent.click(screen.getByRole('button', { name: 'Reject item' }));
    expect(commits.mock.calls).toEqual([[42, key === 'z' ? 'accept' : 'reject']]);
    expect(screen.getByRole('button', { name: 'Accept item' })).toBeDisabled();
    await act(async () => { await vi.advanceTimersByTimeAsync(360); });
    fireEvent.keyDown(window, { key: 'x' });
    expect(commits).toHaveBeenCalledTimes(1);
    expect(onAdvance).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(120); });
    expect(onAdvance).toHaveBeenCalledOnce();

    // Only after advance is the new inbox item reviewable.
    fireEvent.keyDown(window, { key: 'x' });
    expect(commits.mock.calls[1]).toEqual([43, 'reject']);
  });

  it('locks synchronously against multiple shortcuts in the same render', () => {
    const onCommit = vi.fn(() => new Promise<boolean>(() => {}));
    render(<InboxReviewControls itemId={42} onCommit={onCommit} onAdvance={vi.fn()} />);
    act(() => {
      fireEvent.keyDown(window, { key: 'z' });
      fireEvent.keyDown(window, { key: 'x' });
    });
    expect(onCommit).toHaveBeenCalledOnce();
  });
  it('keeps the current item and resets the controls when confirmation is cancelled', async () => {
    let cancel!: () => void;
    const onCommit = vi.fn(() => new Promise<boolean>((resolve) => { cancel = () => resolve(false); }));
    const onAdvance = vi.fn();
    render(<InboxReviewControls itemId={42} onCommit={onCommit} onAdvance={onAdvance} />);
    fireEvent.click(screen.getByRole('button', { name: 'Reject item' }));
    await act(async () => { vi.runAllTimers(); });
    expect(onAdvance).not.toHaveBeenCalled();
    await act(async () => { cancel(); });
    await act(async () => { vi.runAllTimers(); });
    expect(onAdvance).not.toHaveBeenCalled();
    expect(showErrorNotification).not.toHaveBeenCalled();
    expect(document.querySelector('[data-inbox-review-controls]')).toHaveAttribute('data-review-decision', 'idle');
    expect(screen.getByRole('button', { name: 'Reject item' })).toBeEnabled();
    fireEvent.keyDown(window, { key: 'z' });
    expect(onCommit).toHaveBeenCalledTimes(2);
    expect(onCommit).toHaveBeenLastCalledWith(42, 'accept');
  });
});
