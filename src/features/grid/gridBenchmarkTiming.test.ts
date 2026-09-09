import { performance as nodePerformance } from 'node:perf_hooks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

beforeEach(() => { vi.stubGlobal('performance', nodePerformance); });

afterEach(() => {
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
  vi.resetModules();
  performance.clearMeasures('picto:grid-preparation-commit');
  vi.unstubAllGlobals();
});

describe('opt-in grid timing', () => {
  it('does not record in an ordinary package', async () => {
    vi.stubEnv('VITE_BROWSE_BENCHMARK', '');
    const timing = await import('./gridBenchmarkTiming');
    const items: unknown[] = [];
    timing.endGridPreparation(items, timing.beginGridPreparation());
    timing.beginGridCommit(items);
    timing.finishGridCommit(items);
    expect(performance.getEntriesByName('picto:grid-preparation-commit')).toHaveLength(0);
  });

  it('pairs preparation with its own items and records first base draw once', async () => {
    vi.stubEnv('VITE_BROWSE_BENCHMARK', '1');
    const timing = await import('./gridBenchmarkTiming');
    const clock = vi.spyOn(performance, 'now');
    clock.mockReturnValueOnce(100).mockReturnValueOnce(102)
      .mockReturnValueOnce(110).mockReturnValueOnce(115);
    const items = new Array(1500);
    timing.endGridPreparation(items, timing.beginGridPreparation());
    timing.beginGridCommit(items);
    timing.finishGridCommit([]);
    timing.finishGridCommit(items);
    timing.finishGridCommit(items);
    const entries = performance.getEntriesByName('picto:grid-preparation-commit') as PerformanceMeasure[];
    expect(entries).toHaveLength(1);
    expect(entries[0].duration).toBe(7);
    expect(entries[0].detail).toEqual({ items: 1500, preparation_ms: 2, commit_to_base_ms: 5 });
  });
});
