import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { clearDiagnostics, getDiagnosticsSnapshot } from '../features/diagnostics/diagnosticsStore';
import { startDiagnosticsRuntime } from './diagnosticsRuntime';

describe('diagnosticsRuntime', () => {
  beforeEach(() => {
    clearDiagnostics();
    (window as any).picto = {
      api: { invoke: vi.fn() },
      events: { on: vi.fn().mockResolvedValue(vi.fn()) },
    };
  });

  afterEach(() => {
    delete (window as any).picto;
    vi.restoreAllMocks();
  });

  test('captures renderer console output and restores the console on cleanup', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    const wrappedTarget = console.warn;
    const stop = startDiagnosticsRuntime();

    console.warn('slow frame', { gap: 42 });
    expect(getDiagnosticsSnapshot()).toMatchObject([
      {
        level: 'WARN',
        source: 'renderer',
        target: 'renderer.console.warn',
        message: 'slow frame {"gap":42}',
      },
    ]);

    stop();
    expect(console.warn).toBe(wrappedTarget);
  });
});
