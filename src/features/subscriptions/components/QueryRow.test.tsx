import { fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type { SubscriptionQueryInfo } from '../../../shared/types/subscriptions';
import { QueryRow } from './QueryRow';

vi.mock('../../../shared/ui/KbdTooltip/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

const query: SubscriptionQueryInfo = {
  id: '11',
  site_id: 'fanbox',
  query_kind: 'account',
  query_text: 'creator-name',
  display_name: 'Creator',
  notes: null,
  group_posts: true,
  paused: false,
  last_check_time: null,
  files_found: 4,
  posts_found: 2,
  completed_initial_run: true,
  source_history_complete: true,
  successful_run_count: 1,
  resume_cursor: null,
  resume_strategy: null,
  last_success_at: null,
  last_failure_at: null,
  last_failure_kind: null,
  last_failure_message: null,
};

function renderRow() {
  const callbacks = {
    onPause: vi.fn(),
    onRun: vi.fn(),
    onGrouping: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onOpenAuth: vi.fn(),
    onShowStats: vi.fn(),
  };
  render(
    <QueryRow
      query={query}
      sites={[]}
      running={false}
      subscriptionRunning={false}
      paused={false}
      authWarning={null}
      busy={false}
      {...callbacks}
    />,
  );
  return callbacks;
}

describe('QueryRow', () => {
  it('opens source details from row content but not from action controls', () => {
    const callbacks = renderRow();

    fireEvent.doubleClick(screen.getByText('Creator'));
    expect(callbacks.onShowStats).toHaveBeenCalledTimes(1);

    fireEvent.doubleClick(screen.getByLabelText('Run query now'));
    expect(callbacks.onShowStats).toHaveBeenCalledTimes(1);
  });

  it('presents the provider as the source and the configured text as the query', () => {
    renderRow();

    expect(screen.getByText('fanbox')).toHaveAttribute('title', 'fanbox');
    expect(screen.getByText('Creator')).toHaveAttribute('title', 'creator-name');
    expect(screen.queryByText('never')).not.toBeInTheDocument();
  });

  it('runs only this query and describes pausing as putting it on hold', () => {
    const callbacks = renderRow();

    fireEvent.click(screen.getByLabelText('Run query now'));
    fireEvent.click(screen.getByLabelText('Put query on hold'));

    expect(callbacks.onRun).toHaveBeenCalledTimes(1);
    expect(callbacks.onPause).toHaveBeenCalledWith(true);
  });
});
