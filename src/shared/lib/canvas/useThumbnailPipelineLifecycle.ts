import { useEffect, useRef } from 'react';
import { ThumbnailPipeline } from './thumbnailPipeline';

export function useThumbnailPipelineLifecycle(args: {
  markDirty: (lanes: 'base' | 'overlay' | 'both') => void;
  isScrollingRef: { current: boolean };
  pendingAtlasDirtyRef: { current: boolean };
}) {
  const { markDirty, isScrollingRef, pendingAtlasDirtyRef } = args;
  const atlasRef = useRef<ThumbnailPipeline | null>(null);

  useEffect(() => {
    const atlas = new ThumbnailPipeline(() => {
      if (isScrollingRef.current) {
        pendingAtlasDirtyRef.current = true;
        return;
      }
      markDirty('base');
    });
    atlasRef.current = atlas;
    return () => {
      atlas.destroy();
      atlasRef.current = null;
    };
  }, [isScrollingRef, markDirty, pendingAtlasDirtyRef]);

  return atlasRef;
}
