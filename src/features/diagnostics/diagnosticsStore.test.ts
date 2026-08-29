import { beforeEach, describe, expect, test } from 'vitest';
import {
  addDiagnostic,
  clearDiagnostics,
  getDiagnosticsSnapshot,
  recordIpcCall,
} from './diagnosticsStore';

describe('diagnosticsStore', () => {
  beforeEach(clearDiagnostics);

  test('records only slow and failed round trips with useful severity', () => {
    recordIpcCall('items.page', 4.25);
    recordIpcCall('subscriptions.run', 125);
    recordIpcCall('files.export', 8, new Error('disk full'));

    expect(getDiagnosticsSnapshot()).toMatchObject([
      { level: 'WARN', source: 'ipc', target: 'subscriptions.run', durationMs: 125 },
      { level: 'ERROR', source: 'ipc', target: 'files.export', message: 'disk full' },
    ]);
  });

  test('does not log the diagnostics observer itself', () => {
    recordIpcCall('diagnostics.snapshot', 200);
    recordIpcCall('diagnostics.snapshot', 1, new Error('unavailable'));

    expect(getDiagnosticsSnapshot()).toEqual([]);
  });

  test('does not surface expected cloud status polling before a library opens', () => {
    recordIpcCall('cloud.status.get', 3, new Error('No library is open. Call open_library() first.'));
    recordIpcCall('cloud.status.get', 3, new Error('Cloud provider failed'));

    expect(getDiagnosticsSnapshot()).toMatchObject([
      { level: 'ERROR', source: 'ipc', target: 'cloud.status.get', message: 'Cloud provider failed' },
    ]);
  });

  test('clears entries without retaining stale snapshots', () => {
    addDiagnostic({
      level: 'INFO',
      source: 'renderer',
      target: 'test',
      message: 'hello',
      timestamp: new Date(0).toISOString(),
    });
    const before = getDiagnosticsSnapshot();
    clearDiagnostics();

    expect(before).toHaveLength(1);
    expect(getDiagnosticsSnapshot()).toEqual([]);
  });
});
