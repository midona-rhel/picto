import { describe, expect, it } from 'vitest';
import { resolveGridScrollAnchor } from './gridScrollAnchor';

const item = (entity_hash: string) => ({ entity_hash });
const position = (y: number, h = 100) => ({ x: 0, y, w: 100, h });

describe('resolveGridScrollAnchor', () => {
  it('anchors insertion above the viewport by entity hash', () => {
    const next = resolveGridScrollAnchor({
      previousPositions: [position(0), position(110), position(220)],
      nextPositions: [position(0), position(110), position(220), position(330)],
      previousItems: [item('a'), item('b'), item('c')],
      nextItems: [item('new'), item('a'), item('b'), item('c')],
      selectedHashes: new Set(),
      scrollTop: 115,
      viewportHeight: 100,
    });
    expect(next).toBe(225);
  });

  it('anchors a reordered selected entity rather than its old index', () => {
    const next = resolveGridScrollAnchor({
      previousPositions: [position(0), position(110), position(220)],
      nextPositions: [position(0), position(110), position(220)],
      previousItems: [item('a'), item('b'), item('c')],
      nextItems: [item('b'), item('c'), item('a')],
      selectedHashes: new Set(['b']),
      scrollTop: 100,
      viewportHeight: 150,
    });
    expect(next).toBe(0);
  });
});
