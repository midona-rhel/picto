import type { RefObject, SyntheticEvent } from 'react';
import type { ImageSize } from './hooks/useImageZoom';
import { ProgressiveMediaFrame } from './ProgressiveMediaFrame';

type ImageCrossfadeFrameProps = {
  frameRef: RefObject<HTMLDivElement>;
  fullImageRef: RefObject<HTMLImageElement>;
  imageSize: ImageSize | null;
  thumbnailUrl: string;
  fullUrl: string;
  thumbnailVisible: boolean;
  fullVisible: boolean;
  onThumbnailLoad: (event: SyntheticEvent<HTMLImageElement>) => void;
  onFullLoad: (event: SyntheticEvent<HTMLImageElement>) => void;
};

/** One transform owner for both image layers prevents crossfade position drift. */
export function ImageCrossfadeFrame({
  frameRef,
  fullImageRef,
  imageSize,
  thumbnailUrl,
  fullUrl,
  thumbnailVisible,
  fullVisible,
  onThumbnailLoad,
  onFullLoad,
}: ImageCrossfadeFrameProps) {
  const hasAuthoritativeSize = Boolean(imageSize?.width && imageSize?.height);
  const layerStyle = {
    position: 'absolute' as const,
    inset: 0,
    display: 'block',
    width: '100%',
    height: '100%',
    maxWidth: 'none',
    maxHeight: 'none',
    objectFit: 'fill' as const,
    objectPosition: 'center',
  };

  const thumbnail = (
    <img
      data-image-crossfade-layer="thumbnail"
      src={thumbnailUrl}
      alt=""
      draggable={false}
      onLoad={onThumbnailLoad}
      style={layerStyle}
    />
  );

  return (
    <ProgressiveMediaFrame
      frameRef={frameRef}
      preview={thumbnail}
      previewVisible={hasAuthoritativeSize && thumbnailVisible}
      contentReady={hasAuthoritativeSize && fullVisible}
      dataAttributes={{ 'data-image-crossfade-frame': '' }}
      style={{
        position: 'absolute',
        left: '50%',
        top: '50%',
        width: imageSize?.width,
        height: imageSize?.height,
        aspectRatio: hasAuthoritativeSize ? `${imageSize!.width} / ${imageSize!.height}` : undefined,
        overflow: 'hidden',
      }}
      className="image-crossfade-frame"
    >
      {fullUrl && (
        <img
          ref={fullImageRef}
          data-image-crossfade-layer="full"
          src={fullUrl}
          alt=""
          decoding="async"
          draggable={false}
          onLoad={onFullLoad}
          style={layerStyle}
        />
      )}
    </ProgressiveMediaFrame>
  );
}
