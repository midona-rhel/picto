import { atom } from 'jotai';

export interface GridPerfSnapshot {
  fps: number;
  droppedFrames: number;
  nearThresholdFrames: number;
  missedFrames: number;
  pauseFrames: number;
  drawOverBudgetFrames: number;
  avgFrameGapMs: number;
  maxFrameGapMs: number;
  maxMissedFrameGapMs: number;
  maxPauseGapMs: number;
  avgRafDelayMs: number;
  maxRafDelayMs: number;
  totalP99Ms: number;
  slowestPhase: string;
  slowestPhaseP99Ms: number;
  queueDepth: number;
  activeLoads: number;
  cacheEntries: number;
  cacheMb: number;
  visibleTileCount: number;
  visibleUniqueThumbCount: number;
  visibleUniqueThumbReady: number;
  visibleUniqueThumbLoading: number;
  visibleUniqueThumbQueued: number;
  visibleUniqueThumbMissing: number;
  scrollActive: boolean;
  scrollFrames: number;
  avgScrollVelocityPxPerMs: number;
  maxScrollVelocityPxPerMs: number;
  rafFramesWhileIdle: number;
  rafFramesWhileScrolling: number;
  scrollTranslationMode: 'snapped' | 'unsnapped';
  inferredCause: string;
  inferredReason: string;
  updatedAt: number;
}

export const gridPerfAtom = atom<GridPerfSnapshot | null>(null);
