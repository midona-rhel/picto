import { screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { CloudSyncBar } from './CloudSyncBar';

afterEach(() => {
  vi.restoreAllMocks();
});

describe('CloudSyncBar', () => {
  it('shows non-blocking cloud work at the bottom without reflowing the workspace', async () => {
    const invoke = vi.fn().mockResolvedValue({
      state: 'reconciling',
      phase: 'blobs',
      blocking: false,
      completed_units: 4 * 1024 * 1024 * 1024,
      total_units: 12 * 1024 * 1024 * 1024,
      message: 'Restoring library media',
      last_sync_at: null,
      pending_mutations: 0,
      pending_blobs: 800,
      missing_blobs: 0,
    });
    Object.defineProperty(window, 'picto', {
      configurable: true,
      value: { api: { invoke } },
    });

    renderWithProviders(<CloudSyncBar />);

    expect(await screen.findByText('Restoring library media')).toBeInTheDocument();
    expect(screen.getByText('4.00 GB / 12.0 GB')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveAttribute('data-open');
    expect(screen.getByRole('status').firstElementChild).toContainElement(screen.getByRole('progressbar'));
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '33');
  });

  it('moves blocking cloud recovery into the modal progress dialog', async () => {
    const invoke = vi.fn().mockResolvedValue({
      state: 'reconciling',
      phase: 'blobs',
      blocking: true,
      completed_units: 2,
      total_units: 5,
      message: 'Restoring library media',
      last_sync_at: null,
      pending_mutations: 0,
      pending_blobs: 3,
      missing_blobs: 0,
    });
    Object.defineProperty(window, 'picto', {
      configurable: true,
      value: { api: { invoke } },
    });

    renderWithProviders(<CloudSyncBar />);

    const dialog = await screen.findByRole('dialog', { name: 'Restoring library media' });
    expect(within(dialog).getByText('2 B / 5 B')).toBeInTheDocument();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });
});
