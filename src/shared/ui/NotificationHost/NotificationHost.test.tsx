import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clearNotifications,
  showErrorNotification,
  showInfoNotification,
} from '../../lib/notifications';
import { NotificationHost } from './NotificationHost';

describe('NotificationHost', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    act(() => clearNotifications());
  });

  afterEach(() => {
    act(() => clearNotifications());
    vi.useRealTimers();
  });

  it('replaces the current notification instead of stacking cards', () => {
    render(<NotificationHost />);
    act(() => showErrorNotification({ title: 'First', message: 'Old failure' }));
    act(() => showErrorNotification({ title: 'Second', message: 'New failure' }));

    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(screen.getByRole('alert')).toHaveTextContent('Second — New failure');
    expect(screen.queryByText('Old failure')).not.toBeInTheDocument();
  });

  it('uses the four-second reference application duration and supports explicit dismissal', () => {
    render(<NotificationHost />);
    act(() => showInfoNotification({ title: 'Complete', message: 'Finished' }));
    expect(screen.getByRole('status')).toBeInTheDocument();

    act(() => vi.advanceTimersByTime(4_000));
    expect(screen.getByRole('status').className).toContain('hidden');
    act(() => vi.advanceTimersByTime(400));
    expect(screen.queryByRole('status')).not.toBeInTheDocument();

    act(() => showInfoNotification({ title: 'Again', message: 'Dismiss me' }));
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss notification' }));
    act(() => vi.advanceTimersByTime(400));
    expect(screen.queryByText('Dismiss me')).not.toBeInTheDocument();
  });
});
