export type ThumbnailPipelineState = 'idle' | 'shown' | 'error';

export interface ThumbnailPipelineEntry {
  thumb: ImageBitmap | null;
  state: ThumbnailPipelineState;
  lastAccessed: number;
  bytes: number;
  /** Whether this entry should fade in. Set on fresh bitmap load, cleared on eviction/upgrade. */
  animateIn: boolean;
  /** Timestamp (performance.now) when the fade started. 0 = not animating. */
  revealStartedAt: number;
}
