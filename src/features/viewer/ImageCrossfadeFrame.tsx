import type { CSSProperties, RefObject, SyntheticEvent } from 'react';
import type { ImageSize } from './hooks/useImageZoom';
import { ProgressiveMediaFrame } from './ProgressiveMediaFrame';
import styles from './ImageCrossfadeFrame.module.css';

type ImageCrossfadeFrameProps = {
  frameRef: RefObject<HTMLDivElement>;
  fullImageRef: RefObject<HTMLImageElement>;
  imageSize: ImageSize | null;
  thumbnailUrl: string;
  fullUrl: string;
  thumbnailVisible: boolean;
  fullVisible: boolean;
  imageRendering?: 'smooth' | 'pixelated';
  showTransparencyGrid?: boolean;
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
  imageRendering = 'smooth',
  showTransparencyGrid = false,
  onThumbnailLoad,
  onFullLoad,
}: ImageCrossfadeFrameProps) {
  const hasAuthoritativeSize = Boolean(imageSize?.width && imageSize?.height);
  const layerStyle: CSSProperties = {
    position: 'absolute' as const,
    inset: 0,
    display: 'block',
    width: '100%',
    height: '100%',
    maxWidth: 'none',
    maxHeight: 'none',
    objectFit: 'fill' as const,
    objectPosition: 'center',
    imageRendering: imageRendering === 'pixelated' ? 'pixelated' : 'auto',
  };

  const thumbnail = thumbnailUrl ? (
    <img
      key={thumbnailUrl}
      data-image-crossfade-layer="thumbnail"
      data-image-crossfade-thumbnail="displayed"
      src={thumbnailUrl}
      alt=""
      draggable={false}
      onLoad={onThumbnailLoad}
      style={layerStyle}
    />
  ) : null;

  return (
    <ProgressiveMediaFrame
      frameRef={frameRef}
      preview={thumbnail}
      previewVisible={hasAuthoritativeSize && thumbnailVisible}
      instantPreviewWhenContentNotReady
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
      className={`image-crossfade-frame ${showTransparencyGrid ? styles.transparencyGrid : ''}`}
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
