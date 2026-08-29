import { describe, expect, it } from 'vitest';
import type { CloudSyncStatus } from '../../shared/types/generated/application/CloudSyncStatus';
import { estimateRemainingSeconds, formatLastSync, formatRemainingTime, presentCloudSync } from './librarySyncPresentation';

const status = (overrides: Partial<CloudSyncStatus> = {}): CloudSyncStatus => ({
  state: 'idle',
  phase: 'idle',
  blocking: false,
  completed_units: 0,
  total_units: null,
  message: '',
  last_sync_at: null,
  pending_mutations: 0,
  pending_blobs: 0,
  missing_blobs: 0,
  ...overrides,
});

describe('library cloud sync presentation', () => {
  it('keeps queued work idle until reconciliation actually starts', () => {
    expect(presentCloudSync(status({ pending_blobs: 7 }))).toMatchObject({
      active: false,
      label: 'Idle',
      remaining: null,
      workKey: null,
    });
  });

  it('uses exact reconciliation progress when available', () => {
    expect(presentCloudSync(status({
      state: 'reconciling',
      phase: 'applying',
      completed_units: 40,
      total_units: 100,
    }))).toMatchObject({ completed: 40, total: 100, remaining: 60, workKey: 'phase:applying:100' });
  });

  it('describes completed work as idle', () => {
    expect(presentCloudSync(status())).toMatchObject({
      active: false,
      label: 'Idle',
    });
  });

  it('estimates remaining time only from observed progress', () => {
    expect(estimateRemainingSeconds([
      { at: 1_000, remaining: 100 },
      { at: 6_000, remaining: 80 },
    ], 80)).toBe(20);
    expect(estimateRemainingSeconds([{ at: 1_000, remaining: 100 }], 100)).toBeNull();
  });

  it('formats ETA and recent successful syncs for the manager', () => {
    expect(formatRemainingTime(20)).toBe('Less than a minute remaining');
    expect(formatRemainingTime(121)).toBe('About 3 min remaining');
    expect(formatLastSync('2026-08-25T10:00:00Z', Date.parse('2026-08-25T10:20:00Z'))).toBe('20 min ago');
  });
});
