import { describe, expect, it } from 'vitest';
import type { CanonicalTagRecord } from '../../shared/types/canonical';
import { mergeUniqueTagRecords } from './TagSelectModal';

function tag(tagId: number, subname: string): CanonicalTagRecord {
  return {
    tag_id: tagId,
    namespace_id: 1,
    namespace: 'general',
    subname,
    active_count: 1,
    assignment_count: 1,
  };
}

describe('TagSelectModal result merging', () => {
  it('does not duplicate a tag when the same cursor page settles more than once', () => {
    const oneGirl = tag(5, '1girl');
    const firstPage = [oneGirl, tag(6, 'solo')];

    const merged = mergeUniqueTagRecords(
      mergeUniqueTagRecords([], firstPage),
      firstPage,
    );

    expect(merged).toEqual(firstPage);
    expect(merged.filter((item) => item.subname === '1girl')).toHaveLength(1);
  });
});
