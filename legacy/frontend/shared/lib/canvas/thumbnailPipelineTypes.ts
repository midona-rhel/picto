import type { CanvasScrollDirection, CanvasScrollPhase } from './scrollState';

export type ThumbnailPipelineState = 'idle' | 'queued' | 'loading' | 'shown' | 'error';
export type ThumbnailSourceKind = 'thumbnail' | 'full';
export type ThumbnailRequestPriority = 'visible' | 'prefetch';

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
  activeByClass: {
    visibleThumb: number;
    prefetchThumb: number;
    visibleFull: number;
  };
  queuedByClass: {
    visibleThumb: number;
    prefetchThumb: number;
    visibleFull: number;
  };
  cancelCountByClass: {
    visibleThumb: number;
    prefetchThumb: number;
    visibleFull: number;
  };
  visibleThumbWaitMsAvg: number;
  decodeMsAvg: number;
  cacheHitRate: number;
  droppedLateWorkerResults: number;
  scrollPhase: CanvasScrollPhase;
  scrollDirection: CanvasScrollDirection;
  scrollVelocityPxPerSec: number;
}

export interface ThumbnailQueueItem {
  hash: string;
  url: string;
  y: number;
  sourceKind: ThumbnailSourceKind;
  priority: ThumbnailRequestPriority;
  requestedLongEdge: number;
  queuedAt: number;
  resizeWidth?: number;
  resizeHeight?: number;
}

export interface ThumbnailInFlightItem {
  cancel: () => void;
  y: number;
  sourceKind: ThumbnailSourceKind;
  priority: ThumbnailRequestPriority;
  requestedLongEdge: number;
  queuedAt: number;
}
