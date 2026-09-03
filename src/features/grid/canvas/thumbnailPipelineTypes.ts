import type { ThumbnailDecodeQuality } from './thumbnailDecodeClient';

export type ThumbnailPipelineState = 'idle' | 'shown' | 'error';

export interface ThumbnailPipelineEntry {
  thumb: ImageBitmap | null;
  state: ThumbnailPipelineState;
  lastAccessed: number;
  bytes: number;
  quality?: ThumbnailDecodeQuality | null;
}
