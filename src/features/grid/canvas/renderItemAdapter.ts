import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { labToHex } from '../../../shared/lib/labColor';

export interface CanvasRenderItem {
  itemId: number;
  kind: CanonicalEntityGridItem['kind'];
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
  mediaCount: number;
}

export function adaptGridItem(item: CanonicalEntityGridItem): CanvasRenderItem {
  const aspectRatio = item.width && item.height
    ? item.width / item.height
    : item.mime.startsWith('audio/') ? 2 : null;

  return {
    itemId: item.root_id,
    kind: item.kind,
    displayFileHash: item.content_hash,
    hash: String(item.root_id),
    thumbnailHash: item.content_hash,
    name: item.name,
    mime: item.mime,
    width: item.width,
    height: item.height,
    rating: ['unrated', 'one', 'two', 'three', 'four', 'five'].indexOf(item.rating) || null,
    durationMs: item.duration_ms,
    dominantColor: labToHex(item.palette[0]),
    aspectRatio,
    numFrames: item.frame_count,
    mediaCount: item.media_count,
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
  const index = sourceItems.findIndex((item) => item.root_id === itemId);
  return index < 0 ? null : { index, item: sourceItems[index] };
}
