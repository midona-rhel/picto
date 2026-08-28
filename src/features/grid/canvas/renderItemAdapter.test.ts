import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { adaptGridItem, resolveRenderedGridItem } from './renderItemAdapter';

function item(
  itemId: number,
  displayFileHash: string,
  kind: CanonicalEntityGridItem['kind'] = 'media',
): CanonicalEntityGridItem {
  return {
    root_id: itemId,
    kind,
    lifecycle: 'active',
    name: '',
    cover_media_id: itemId,
    content_hash: displayFileHash,
    mime: 'image/png',
    width: 100,
    height: 100,
    duration_ms: null,
    frame_count: null,
    palette: [],
    imported_at_ms: itemId,
    captured_at_ms: null,
    modified_at_ms: itemId,
    rating: 'unrated',
    media_count: 1,
    total_size_bytes: 100,
  };
}

describe('adaptGridItem', () => {
  it('keeps item identity separate from the physical display file', () => {
    const first = adaptGridItem(item(10, 'shared-file'));
    const second = adaptGridItem(item(11, 'shared-file'));

    expect(first.itemId).toBe(10);
    expect(second.itemId).toBe(11);
    expect(first.displayFileHash).toBe('shared-file');
    expect(second.displayFileHash).toBe('shared-file');
    expect(first.mime).toBe('image/png');
  });

  it('carries group identity and membership count into the paint model', () => {
    const group = item(20, 'cover-file', 'collection');
    group.media_count = 37;

    expect(adaptGridItem(group)).toMatchObject({ kind: 'collection', mediaCount: 37 });
  });

  it('resolves interactions from painted identity instead of source-array position', () => {
    const media = item(10, 'media-file');
    const group = item(20, 'cover-file', 'collection');
    const painted = [adaptGridItem(media), adaptGridItem(group)];

    expect(resolveRenderedGridItem(painted, [group, media], 0)).toEqual({
      index: 1,
      item: media,
    });
    expect(resolveRenderedGridItem(painted, [group, media], 1)).toEqual({
      index: 0,
      item: group,
    });
  });

  it('gives audio waveform tiles two-to-one geometry', () => {
    const audio = item(30, 'audio-file');
    audio.mime = 'audio/mpeg';
    audio.width = null;
    audio.height = null;

    expect(adaptGridItem(audio).aspectRatio).toBe(2);
  });
});
