import { useEffect, useRef, type MutableRefObject } from 'react';
import { ThumbnailPipeline } from './thumbnailPipeline';
import type { CanvasScrollPhase } from './scrollState';

export function useThumbnailPipelineLifecycle(args: {
  markDirty: (lanes: 'base' | 'overlay' | 'both') => void;
  scrollPhaseRef: { current: CanvasScrollPhase };
  pendingAtlasDirtyRef: { current: boolean };
  sharedAtlasRef?: MutableRefObject<ThumbnailPipeline | null>;
  destroyOnUnmount?: boolean;
}) {
  const {
    markDirty,
    scrollPhaseRef,
    pendingAtlasDirtyRef,
    sharedAtlasRef,
    destroyOnUnmount = true,
  } = args;
  const internalAtlasRef = useRef<ThumbnailPipeline | null>(null);
  const atlasRef = sharedAtlasRef ?? internalAtlasRef;
  const dirtyRafRef = useRef(0);

  useEffect(() => {
    const atlas = atlasRef.current ?? new ThumbnailPipeline();
    atlasRef.current = atlas;
    atlas.setOnDirty(() => {
      if (scrollPhaseRef.current !== 'idle') {
        pendingAtlasDirtyRef.current = true;
      }
      if (dirtyRafRef.current) return;
      dirtyRafRef.current = requestAnimationFrame(() => {
        dirtyRafRef.current = 0;
        markDirty('base');
      });
    });
    return () => {
      if (dirtyRafRef.current) {
        cancelAnimationFrame(dirtyRafRef.current);
        dirtyRafRef.current = 0;
      }
      atlas.setOnDirty(() => {});
      if (destroyOnUnmount && atlasRef.current === atlas) {
        atlas.destroy();
        atlasRef.current = null;
      }
    };
  }, [atlasRef, destroyOnUnmount, markDirty, pendingAtlasDirtyRef, scrollPhaseRef]);

  return atlasRef;
}
