export type ThumbnailPipelineState = 'idle' | 'queued' | 'loading' | 'shown' | 'error';

export interface ThumbnailPipelineEntry {
  thumb: ImageBitmap | null;
  state: ThumbnailPipelineState;
  lastAccessed: number;
  revealStartedAt: number;
  animateIn: boolean;
  retryQueued: boolean;
}

export interface ThumbnailPipelineStats {
  queueDepth: number;
  activeLoads: number;
  pendingThumbs: number;
  cacheSize: number;
  diskSpeed: 'normal' | 'fast';
}

export interface ThumbnailQueueItem {
  hash: string;
  url: string;
  y: number;
  mime: string;
  targetW: number;
  targetH: number;
}
