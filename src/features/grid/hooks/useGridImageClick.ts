import { useCallback } from 'react';
import { api } from '#desktop/api';
import { prefetchMetadata } from '../metadataPrefetch';
import type { MediaItem } from '../shared';
import type { GridRuntimeAction } from '../runtime';

export function useGridImageClick(args: {
  dispatch: React.Dispatch<GridRuntimeAction>;
  viewer: { openDetail: (hash: string) => void };
  stateRef: { current: {
    images: MediaItem[];
    selectedHashes: Set<string>;
    virtualAllSelection: { excludedHashes: Set<string> } | null;
  } };
  imagesRef: { current: MediaItem[] };
  lastClickedHashRef: { current: string | null };
  canvasLayoutRef: { current: { x: number; y: number; w: number; h: number }[] };
}) {
  const { dispatch, viewer, stateRef, imagesRef, lastClickedHashRef, canvasLayoutRef } = args;

  const recordImageView = useCallback((hash: string) => {
    const image = stateRef.current.images.find((img) => img.hash === hash);
    if (!image || image.is_collection) return;
    void api.files.incrementViewCount(hash).catch((err) => {
      console.warn('Failed to increment view count:', err);
    });
  }, [stateRef]);

  const handleImageClick = useCallback((image: MediaItem, event: React.MouseEvent) => {
    if (event.detail === 2) {
      viewer.openDetail(image.hash);
      return;
    }

    prefetchMetadata(image.hash);
    const { virtualAllSelection } = stateRef.current;
    if (virtualAllSelection) {
      if (event.metaKey || event.ctrlKey) {
        dispatch({ type: 'TOGGLE_VIRTUAL_EXCLUSION', hash: image.hash });
        dispatch({ type: 'SET_LAST_CLICKED', hash: image.hash });
        return;
      }
      dispatch({ type: 'DEACTIVATE_VIRTUAL_SELECT_ALL' });
    }

    if (event.metaKey || event.ctrlKey) {
      dispatch({ type: 'TOGGLE_HASH', hash: image.hash });
    } else if (event.shiftKey && lastClickedHashRef.current) {
      const positions = canvasLayoutRef.current;
      const currentImages = imagesRef.current;
      const prevSelected = stateRef.current.selectedHashes;
      if (positions.length > 0) {
        const indices = Array.from({ length: Math.min(positions.length, currentImages.length) }, (_, i) => i);
        indices.sort((a, b) => {
          const pa = positions[a];
          const pb = positions[b];
          const dy = pa.y - pb.y;
          if (Math.abs(dy) > pa.h * 0.5) return dy;
          return pa.x - pb.x;
        });
        const visualHashes = indices.map((i) => currentImages[i].hash);
        const startIdx = visualHashes.indexOf(lastClickedHashRef.current);
        const endIdx = visualHashes.indexOf(image.hash);
        if (startIdx !== -1 && endIdx !== -1) {
          const [lo, hi] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
          const next = new Set(prevSelected);
          for (let i = lo; i <= hi; i++) next.add(visualHashes[i]);
          dispatch({ type: 'SELECT_HASHES', hashes: next });
          dispatch({ type: 'SET_LAST_CLICKED', hash: image.hash });
          return;
        }
      }
      const startIdx = currentImages.findIndex((i) => i.hash === lastClickedHashRef.current);
      const endIdx = currentImages.findIndex((i) => i.hash === image.hash);
      const [lo, hi] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
      const next = new Set(prevSelected);
      for (let i = lo; i <= hi; i++) next.add(currentImages[i].hash);
      dispatch({ type: 'SELECT_HASHES', hashes: next });
    } else {
      dispatch({ type: 'SELECT_HASHES', hashes: new Set([image.hash]) });
    }
    dispatch({ type: 'SET_LAST_CLICKED', hash: image.hash });
  }, [canvasLayoutRef, dispatch, imagesRef, lastClickedHashRef, stateRef, viewer]);

  return {
    recordImageView,
    handleImageClick,
  };
}
