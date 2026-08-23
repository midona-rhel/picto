import { describe, expect, it } from 'vitest';
import { planFolderReorder } from './folderReorder';

describe('planFolderReorder', () => {
  it('moves a selection between stationary anchors without rewriting other ranks', () => {
    expect(planFolderReorder(
      ['a', 'b', 'c', 'd', 'e'],
      new Set(['b', 'c']),
      4,
      'left',
    )).toEqual([
      { hash: 'b', after_hash: 'd', before_hash: 'e' },
      { hash: 'c', after_hash: 'b', before_hash: 'e' },
    ]);
  });

  it('anchors a move at the beginning of the loaded window', () => {
    expect(planFolderReorder(
      ['a', 'b', 'c', 'd', 'e'],
      new Set(['d', 'e']),
      0,
      'left',
    )).toEqual([
      { hash: 'd', after_hash: null, before_hash: 'a' },
      { hash: 'e', after_hash: 'd', before_hash: 'a' },
    ]);
  });

  it('chains a move after the last loaded stationary entity', () => {
    expect(planFolderReorder(
      ['a', 'b', 'c', 'd', 'e'],
      new Set(['a', 'b']),
      4,
      'right',
    )).toEqual([
      { hash: 'a', after_hash: 'e', before_hash: null },
      { hash: 'b', after_hash: 'a', before_hash: null },
    ]);
  });

  it('does not emit a backend write for a no-op drop', () => {
    expect(planFolderReorder(
      ['a', 'b', 'c', 'd'],
      new Set(['b', 'c']),
      3,
      'left',
    )).toEqual([]);
  });
});
