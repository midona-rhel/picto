import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';

export interface CanvasRenderItem {
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
    hash: item.entity_hash,
    thumbnailHash: item.entity_hash,
    name: item.name,
    mime: item.mime_type,
    width: item.pixel_width,
    height: item.pixel_height,
    rating: item.rating,
    durationMs: item.duration_ms,
    dominantColor: item.dominant_color_hex,
    aspectRatio,
    numFrames: item.frame_count,
  };
}
