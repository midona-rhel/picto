import { useRef, useState, type CSSProperties, type RefObject, type SyntheticEvent } from 'react';
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
  const [paintedThumbnailUrl, setPaintedThumbnailUrl] = useState(thumbnailUrl);
  const requestedThumbnailUrlRef = useRef(thumbnailUrl);
  requestedThumbnailUrlRef.current = thumbnailUrl;
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

  const thumbnailChanging = paintedThumbnailUrl !== thumbnailUrl;
  const thumbnail = (
    <>
      {paintedThumbnailUrl && (
        <img
          key={paintedThumbnailUrl}
          data-image-crossfade-layer="thumbnail"
          data-image-crossfade-thumbnail="painted"
          src={paintedThumbnailUrl}
          alt=""
          draggable={false}
          onLoad={thumbnailChanging ? undefined : onThumbnailLoad}
          style={layerStyle}
        />
      )}
      {thumbnailChanging && thumbnailUrl && (
        <img
          key={thumbnailUrl}
          data-image-crossfade-layer="thumbnail"
          data-image-crossfade-thumbnail="incoming"
          src={thumbnailUrl}
          alt=""
          draggable={false}
          onLoad={(event) => {
            const image = event.currentTarget;
            const requestedUrl = thumbnailUrl;
            onThumbnailLoad(event);
            const commit = () => {
              if (requestedThumbnailUrlRef.current === requestedUrl) {
                setPaintedThumbnailUrl(requestedUrl);
              }
            };
            if (typeof image.decode === 'function') image.decode().then(commit).catch(commit);
            else commit();
          }}
          style={{ ...layerStyle, opacity: 0 }}
        />
      )}
    </>
  );

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
