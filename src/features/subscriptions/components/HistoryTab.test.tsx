import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { SubscriptionRunRecord } from '../../../shared/types/subscriptions';
import styles from '../SubscriptionsScreen.module.css';
import { HistoryTab } from './HistoryTab';

const run: SubscriptionRunRecord = {
  run_id: 1,
  subscription_id: 1,
  started_at: '2026-08-23T10:00:00Z',
  finished_at: '2026-08-23T10:01:00Z',
  status: 'completed',
  posts_traversed: 6,
  posts_added: 3,
  posts_skipped: 3,
  files_already_in_library: 7,
  media_added: 4,
  files_downloaded: 4,
  files_skipped: 2,
  metadata_validated: 4,
  metadata_invalid: 0,
  failure_kind: null,
  error_message: null,
};

describe('HistoryTab', () => {
  it('uses the shared subscription table/header primitives and a single Reused heading', () => {
    const { container } = render(<HistoryTab runs={[run]} />);

    expect(container.firstElementChild).toHaveClass(styles.subscriptionTable, styles.historyTable);
    expect(screen.getByText('Started').parentElement).toHaveClass(
      styles.subscriptionTableRow,
      styles.subscriptionTableHeader,
      styles.historyRow,
    );
    expect(screen.getByText('Reused')).toBeInTheDocument();
    expect(screen.getByTitle('Files already in library')).toHaveTextContent('7');
    expect(screen.queryByText('Reused duplicate')).not.toBeInTheDocument();
  });

  it('shows an inbox-cap wait as a paused history state with its persisted reason', () => {
    render(<HistoryTab runs={[{
      ...run,
      status: 'pending',
      finished_at: null,
      failure_kind: 'inbox_full',
      error_message: 'Stopped because Inbox reached its limit of 1,000 items.',
    }]} />);

    expect(screen.getByText('Inbox full')).toHaveAttribute(
      'title',
      'Stopped because Inbox reached its limit of 1,000 items.',
    );
  });
});
