import type { GridDebugStats } from './canvasGridDebug';

export function CanvasGridDebugHud({ debugStats }: { debugStats: GridDebugStats }) {
  return (
    <div
      style={{
        position: 'fixed',
        right: 12,
        bottom: 12,
        zIndex: 200100,
        pointerEvents: 'none',
        background: 'var(--color-black-70)',
        color: 'var(--color-text-primary)',
        border: '1px solid var(--color-white-20)',
        borderRadius: 8,
        padding: '8px 10px',
        fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
        fontSize: 'var(--font-size-2xs)',
        lineHeight: 'var(--line-height-normal)',
        minWidth: 240,
        whiteSpace: 'pre',
      }}
    >
{`fps ${debugStats.fps.toFixed(1)}  draw ${debugStats.drawMs.toFixed(2)}ms  vis ${debugStats.visMs.toFixed(2)}ms
tiles vis ${debugStats.visibleTiles}  prefetch ${debugStats.prefetchedTiles}
atlas q ${debugStats.queueDepth}  active ${debugStats.activeLoads}  pending ${debugStats.pendingThumbs}
cache ${debugStats.cacheSize}  slowFrames ${debugStats.slowFrames}  disk ${debugStats.diskSpeed}
base ${debugStats.baseRedraws}  overlay ${debugStats.overlayRedraws}`}
    </div>
  );
}
