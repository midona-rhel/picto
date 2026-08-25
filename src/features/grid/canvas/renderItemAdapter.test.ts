import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { adaptGridItem, resolveRenderedGridItem } from './renderItemAdapter';

function item(
  itemId: number,
  displayFileHash: string,
  kind: CanonicalEntityGridItem['kind'] = 'media',
): CanonicalEntityGridItem {
  return {
    item_id: itemId,
    kind,
    lifecycle: 'active',
    name: null,
    display_file_hash: displayFileHash,
    display_mime_type: 'image/png',
    pixel_width: 100,
    pixel_height: 100,
    duration_ms: null,
    frame_count: null,
    dominant_color_hex: null,
    rating: null,
    media_count: 1,
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

  it('gives audio waveform tiles reference application\'s two-to-one geometry', () => {
    const audio = item(30, 'audio-file');
    audio.display_mime_type = 'audio/mpeg';
    audio.pixel_width = null;
    audio.pixel_height = null;

    expect(adaptGridItem(audio).aspectRatio).toBe(2);
  });
});
