import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { adaptGridItem } from './renderItemAdapter';

function item(itemId: number, displayFileHash: string): CanonicalEntityGridItem {
  return {
    item_id: itemId,
    kind: 'media',
    lifecycle: 'active',
    label: null,
    name: null,
    display_media_item_id: itemId,
    display_file_hash: displayFileHash,
    display_mime_type: 'image/png',
    pixel_width: 100,
    pixel_height: 100,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    dominant_color_hex: null,
    size_bytes: 1,
    rating: null,
    captured_at: null,
    imported_at: null,
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
});
