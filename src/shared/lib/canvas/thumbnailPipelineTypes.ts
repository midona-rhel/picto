export type ThumbnailPipelineState = 'idle' | 'queued' | 'loading' | 'shown' | 'error';
export type ThumbnailSourceKind = 'thumbnail' | 'full';

export interface ThumbnailPipelineEntry {
  thumb: ImageBitmap | null;
  state: ThumbnailPipelineState;
  lastAccessed: number;
  revealStartedAt: number;
  animateIn: boolean;
  retryQueued: boolean;
  sourceKind: ThumbnailSourceKind;
  loadedLongEdge: number;
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
  sourceKind: ThumbnailSourceKind;
  requestedLongEdge: number;
  resizeWidth?: number;
  resizeHeight?: number;
}

export interface ThumbnailInFlightItem {
  controller: AbortController;
  y: number;
  sourceKind: ThumbnailSourceKind;
  requestedLongEdge: number;
}
