import { render, screen, within } from '@testing-library/react';
import type { ComponentProps, ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { EMPTY_SUBSCRIPTION_DETAIL_STATE } from '../../../state/subscriptionsWorkspace';
import { SubscriptionDetail } from './SubscriptionDetail';

vi.mock('../../../shared/ui/KbdTooltip/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

type Props = ComponentProps<typeof SubscriptionDetail>;
const run = {
  run_id: 1, subscription_id: 1, started_at: '2026-09-01T10:00:00Z', finished_at: null,
  status: 'pending', failure_kind: null, error_message: null,
  posts_traversed: 143, posts_added: 0, posts_skipped: 143,
  files_already_in_library: 189, files_downloaded: 0, media_added: 0,
  files_skipped: 0, metadata_validated: 0, metadata_invalid: 0,
};
const props: Props = {
  subscription: {
    id: '1', name: 'Example', schedule: 'daily', paused: false, run_status: 'paused',
    active_run_id: 1, created_at: '', total_items: 189, posts_per_run: 500,
    target_folder_ids: [], automatic_tags: [], queries: [],
  },
  snapshot: {
    subscriptions: [], globalPaused: false, sites: [], credentials: [], credentialHealth: [],
    runningSubscriptionIds: [], runningProgress: [], listMetrics: {},
  },
  progress: null,
  detail: { ...EMPTY_SUBSCRIPTION_DETAIL_STATE, subscriptionId: '1', runs: [run] },
  busy: false,
  controller: {
    run: vi.fn(), stop: vi.fn(), pauseRun: vi.fn(), resumeRun: vi.fn(), hold: vi.fn(),
    delete: vi.fn(), rename: vi.fn(), setSchedule: vi.fn(), setPostsPerRun: vi.fn(),
    setDestination: vi.fn(), pauseQuery: vi.fn(), runQuery: vi.fn(), setQueryGrouping: vi.fn(),
    deleteQuery: vi.fn(), editQuery: vi.fn(), addQuery: vi.fn(), openExternalUrl: vi.fn(),
  },
  onOpenAccounts: vi.fn(), onLoadMoreHealth: vi.fn(), onOpenMenu: vi.fn(),
};

function expectProperty(label: string, value: string) {
  const row = screen.getByText(label).parentElement!;
  expect(within(row).getByText(value)).toBeInTheDocument();
}

describe('subscription progress counts', () => {
  it('distinguishes existing files from skipped posts and downloads after pausing', () => {
    render(<SubscriptionDetail {...props} />);
    expectProperty('Posts skipped', '143');
    expectProperty('Files already in library', '189');
    expectProperty('Files downloaded', '0');
  });

  it('uses current persisted progress when the run resumes', () => {
    render(<SubscriptionDetail {...props} progress={{
      subscription_id: '1', subscription_name: 'Example', mode: 'manual',
      posts_traversed: 144, posts_added: 0, posts_skipped: 144,
      files_already_in_library: 192, files_downloaded: 0, files_skipped: 0,
      queued_for_ingest: 0, ingesting: 0, media_added: 0, reused: 0, failed_ingest: 0,
      pages_fetched: 0, metadata_validated: 0, metadata_invalid: 0,
      status_text: 'running', current_post_items: 0,
    }} />);
    expectProperty('Files already in library', '192');
    expectProperty('Posts skipped', '144');
    expectProperty('Files downloaded', '0');
  });
});
