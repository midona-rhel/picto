import type { CanvasScrollDirection, CanvasScrollPhase } from './scrollState';

export type ThumbnailPipelineState = 'idle' | 'queued' | 'loading' | 'ready_pending' | 'shown' | 'error';
export type ThumbnailRequestPriority = 'visible' | 'prefetch';

export interface ThumbnailPipelineEntry {
  thumb: ImageBitmap | null;
  state: ThumbnailPipelineState;
  lastAccessed: number;
  revealStartedAt: number;
  animateIn: boolean;
  bytes: number;
}

export interface ThumbnailQueueItem {
  hash: string;
  url: string;
  y: number;
  priority: ThumbnailRequestPriority;
  requestedLongEdge: number;
  queuedAt: number;
  generation: number;
}

export interface ThumbnailInFlightItem {
  cancel: () => void;
  y: number;
  priority: ThumbnailRequestPriority;
  requestedLongEdge: number;
  queuedAt: number;
  generation: number;
}

export interface ThumbnailPipelineStats {
  queueDepth: number;
  activeLoads: number;
  cacheEntries: number;
  totalBytes: number;
  scrollPhase: CanvasScrollPhase;
  scrollDirection: CanvasScrollDirection;
  scrollVelocityPxPerSec: number;
  droppedLateWorkerResults: number;
}

export interface ThumbnailPipelineTraceEvent {
  type: string;
  payload: Record<string, unknown>;
}
