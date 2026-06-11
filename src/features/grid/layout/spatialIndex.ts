/**
 * Y-binned spatial index over layout positions.
 *
 * Masonry (waterfall) positions are not Y-sorted, so range queries over the
 * raw positions array require a full scan. This index buckets tile indices
 * into fixed-height Y slabs so viewport-band queries (activation zone,
 * marquee hit-testing) only touch tiles near the band. 1D is sufficient:
 * callers filter by a Y band first, then run their precise X/Y rect test
 * on the candidates.
 */

import type { LayoutItem } from './types';

/** ≈ one tile row at default sizes. */
const BIN_HEIGHT = 256;

export interface TileSpatialIndex {
  /**
   * Append indices of tiles whose Y extent may overlap [top, bottom] to
   * `out`, deduped and ascending. Returns `out`. Results are a superset on
   * the X axis — callers must still run a precise rect test.
   */
  queryYRange(top: number, bottom: number, out: number[]): number[];
}

const EMPTY_INDEX: TileSpatialIndex = {
  queryYRange(_top: number, _bottom: number, out: number[]): number[] {
    return out;
  },
};

export function buildTileSpatialIndex(positions: ReadonlyArray<LayoutItem | undefined>): TileSpatialIndex {
  if (positions.length === 0) return EMPTY_INDEX;

  let maxY = 0;
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    if (!pos) continue;
    const bottom = pos.y + pos.h;
    if (bottom > maxY) maxY = bottom;
  }

  const binCount = Math.max(1, Math.ceil(maxY / BIN_HEIGHT));
  const bins: number[][] = new Array(binCount);
  for (let b = 0; b < binCount; b++) bins[b] = [];

  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    if (!pos) continue;
    const first = Math.max(0, Math.min(binCount - 1, Math.floor(pos.y / BIN_HEIGHT)));
    const last = Math.max(0, Math.min(binCount - 1, Math.floor((pos.y + pos.h) / BIN_HEIGHT)));
    for (let b = first; b <= last; b++) bins[b].push(i);
  }

  // Generation-stamped visited array — dedup across bins without per-query
  // allocation (tiles spanning multiple slabs appear in each).
  const visited = new Int32Array(positions.length);
  let generation = 0;

  return {
    queryYRange(top: number, bottom: number, out: number[]): number[] {
      if (bottom < 0 || top > maxY) return out;
      generation++;
      const first = Math.max(0, Math.min(binCount - 1, Math.floor(top / BIN_HEIGHT)));
      const last = Math.max(0, Math.min(binCount - 1, Math.floor(bottom / BIN_HEIGHT)));
      const start = out.length;
      for (let b = first; b <= last; b++) {
        const bin = bins[b];
        for (let k = 0; k < bin.length; k++) {
          const i = bin[k];
          if (visited[i] === generation) continue;
          visited[i] = generation;
          out.push(i);
        }
      }
      // Ascending index order keeps consumers deterministic — the thumbnail
      // plan fingerprint depends on a stable tile order per scroll position.
      if (out.length - start > 1) {
        const slice = out.slice(start).sort((a, b) => a - b);
        for (let k = 0; k < slice.length; k++) out[start + k] = slice[k];
      }
      return out;
    },
  };
}
