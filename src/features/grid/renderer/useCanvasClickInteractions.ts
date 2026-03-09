import { useCallback } from 'react';
import { isVideoMime, type MasonryImageItem } from '../shared';

export function useCanvasClickInteractions(args: {
  hitTest: (clientX: number, clientY: number) => number | null;
  isZoomButtonHit: (clientX: number, clientY: number, tileIdx: number) => boolean;
  imagesRef: { current: MasonryImageItem[] };
  onImageClickRef: { current: (image: MasonryImageItem, event: React.MouseEvent) => void };
  showHoverPreview: (image: MasonryImageItem | undefined) => void;
}) {
  const { hitTest, isZoomButtonHit, imagesRef, onImageClickRef, showHoverPreview } = args;

  const handleClick = useCallback((e: React.MouseEvent) => {
    const idx = hitTest(e.clientX, e.clientY);
    if (idx == null) return;
    const image = imagesRef.current[idx];
    if (!image) return;

    if (isZoomButtonHit(e.clientX, e.clientY, idx)) {
      if (!isVideoMime(image.mime) && !image.is_collection) showHoverPreview(image);
      return;
    }

    onImageClickRef.current(image, e);
  }, [hitTest, imagesRef, isZoomButtonHit, onImageClickRef, showHoverPreview]);

  return {
    handleClick,
  };
}
