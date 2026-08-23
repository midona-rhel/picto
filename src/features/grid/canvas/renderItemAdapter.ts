import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';

export interface CanvasRenderItem {
  itemId: number;
  displayFileHash: string;
  // Compatibility keys for the activation/reveal helpers. `hash` is the
  // logical item identity; `thumbnailHash` is the physical file identity.
  hash: string;
  thumbnailHash: string;
  name: string | null;
  mime: string;
  width: number | null;
  height: number | null;
  rating: number | null;
  durationMs: number | null;
  dominantColor: string | null;
  aspectRatio: number | null;
  numFrames: number | null;
}

export function adaptGridItem(item: CanonicalEntityGridItem): CanvasRenderItem {
  const aspectRatio = item.pixel_width && item.pixel_height
    ? item.pixel_width / item.pixel_height
    : null;

  return {
    itemId: item.item_id,
    displayFileHash: item.display_file_hash,
    hash: String(item.item_id),
    thumbnailHash: item.display_file_hash,
    name: item.name,
    mime: item.display_mime_type,
    width: item.pixel_width,
    height: item.pixel_height,
    rating: item.rating,
    durationMs: item.duration_ms,
    dominantColor: item.dominant_color_hex,
    aspectRatio,
    numFrames: item.frame_count,
  };
}

/** Resolve an interaction against the item identity painted at this layout index. */
export function resolveRenderedGridItem(
  renderedItems: readonly CanvasRenderItem[],
  sourceItems: readonly CanonicalEntityGridItem[],
  renderedIndex: number,
): { index: number; item: CanonicalEntityGridItem } | null {
  const itemId = renderedItems[renderedIndex]?.itemId;
  if (itemId == null) return null;
  const index = sourceItems.findIndex((item) => item.item_id === itemId);
  return index < 0 ? null : { index, item: sourceItems[index] };
}
