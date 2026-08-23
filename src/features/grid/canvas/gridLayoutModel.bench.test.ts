import { describe, expect, it } from 'vitest';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode } from '../layout/types';
import { GridLayoutRuntime } from './gridLayoutModel';

const enabled = process.env.GRID_LAYOUT_BENCHMARK === '1';
const config = {
  width: 1440,
  targetSize: 220,
  gap: 16,
  textHeight: 20,
  scrollbarWidth: 8,
};

function items(count: number): CanonicalEntityGridItem[] {
  return Array.from({ length: count }, (_, index) => ({
    entity_hash: index.toString(16).padStart(64, '0'),
    name: String(index),
    mime_type: 'image/jpeg',
    pixel_width: 400 + (index % 13) * 53,
    pixel_height: 300 + (index % 17) * 41,
  } as CanonicalEntityGridItem));
}

function p95(values: number[]) {
  return [...values].sort((a, b) => a - b)[Math.ceil(values.length * 0.95) - 1];
}

describe.runIf(enabled)('grid layout performance probe', () => {
  it('reports full builds and 500-item appends', { timeout: 120_000 }, () => {
    const modes: GridViewMode[] = ['grid', 'waterfall', 'justified'];
    for (const count of [500, 5_000, 25_000, 100_000]) {
      const base = items(count);
      const appended = [...base, ...items(500).map((item, index) => ({
        ...item,
        entity_hash: (count + index).toString(16).padStart(64, '0'),
      }))];
      for (const viewMode of modes) {
        const builds: number[] = [];
        const appends: number[] = [];
        for (let sample = 0; sample < 7; sample++) {
          let started = performance.now();
          new GridLayoutRuntime().update(base, { ...config, viewMode });
          builds.push(performance.now() - started);

          const runtime = new GridLayoutRuntime();
          runtime.update(base, { ...config, viewMode });
          started = performance.now();
          runtime.update(appended, { ...config, viewMode });
          appends.push(performance.now() - started);
        }
        const append500P95 = p95(appends);
        console.info(JSON.stringify({ count, viewMode, buildP95: p95(builds), append500P95 }));
        if (count === 100_000) expect(append500P95).toBeLessThan(8);
      }
    }
  });
});
