import { describe, expect, it } from 'vitest';
import { buildBatchRenamePreview } from './BatchRenameModal';

const items = [
  { item_id: 10, name: 'Fox' },
  { item_id: 11, name: 'Wolf' },
];

describe('buildBatchRenamePreview', () => {
  it('keeps original names and pads sequence tokens', () => {
    expect(buildBatchRenamePreview(items, 'format', '* - %NN', '', 7)).toEqual([
      { item_id: 10, name: 'Fox - 07' },
      { item_id: 11, name: 'Wolf - 08' },
    ]);
  });

  it('replaces every matching fragment', () => {
    expect(buildBatchRenamePreview(items, 'replace', 'o', '0', 1)).toEqual([
      { item_id: 10, name: 'F0x' },
      { item_id: 11, name: 'W0lf' },
    ]);
  });

  it('does not mutate names for an empty replacement search', () => {
    expect(buildBatchRenamePreview(items, 'replace', '', 'x', 1)).toEqual(items);
  });
});
