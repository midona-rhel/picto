import { screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { HeavyJobBar } from './HeavyJobBar';

afterEach(() => {
  vi.restoreAllMocks();
});

describe('HeavyJobBar', () => {
  it('shows byte-accurate cloud recovery without reflowing the workspace', async () => {
    const invoke = vi.fn().mockResolvedValue({
      state: 'reconciling',
      phase: 'blobs',
      blocking: true,
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

    renderWithProviders(<HeavyJobBar />);

    expect(await screen.findByText('Restoring library media')).toBeInTheDocument();
    expect(screen.getByText('4.00 GB / 12.0 GB')).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveAttribute('data-open');
  });
});
