export interface ThumbnailPipelineEntry {
  thumb: ImageBitmap | null;
  quality: 'none' | 'thumb';
  thumbRequested: boolean;
  thumbLoading: boolean;
  thumbRequestedAt: number;
  createdAt: number;
  lastAccessed: number;
  revealStartedAt: number;
  animateIn: boolean;
  error: boolean;
  repairQueued: boolean;
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
