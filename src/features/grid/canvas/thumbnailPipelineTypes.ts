export type ThumbnailPipelineState = 'idle' | 'shown' | 'error';

export interface ThumbnailPipelineEntry {
  thumb: ImageBitmap | null;
  state: ThumbnailPipelineState;
  lastAccessed: number;
  bytes: number;
}
