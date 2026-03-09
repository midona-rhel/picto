export interface GridDebugStats {
  fps: number;
  drawMs: number;
  visMs: number;
  visibleTiles: number;
  prefetchedTiles: number;
  queueDepth: number;
  activeLoads: number;
  pendingThumbs: number;
  cacheSize: number;
  slowFrames: number;
  diskSpeed: 'normal' | 'fast';
  baseRedraws: number;
  overlayRedraws: number;
}
