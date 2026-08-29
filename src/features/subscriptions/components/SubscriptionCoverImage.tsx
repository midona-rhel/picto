import { useEffect, useState } from 'react';
import { mediaFileUrl, mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import { ThumbnailImage } from '../../../shared/ui/ThumbnailImage/ThumbnailImage';

export type SubscriptionCoverCrop = {
  focusX: number;
  focusY: number;
  zoomPercent: number;
};

export type SubscriptionCoverDimensions = { width: number; height: number };

export function SubscriptionCoverThumbnail({
  fileHash,
  className,
  alt = '',
  loading,
  draggable = false,
  onError,
}: {
  fileHash: string;
  className?: string;
  alt?: string;
  loading?: 'eager' | 'lazy';
  draggable?: boolean;
  onError?: () => void;
}) {
  return (
    <ThumbnailImage
      src={mediaThumbnailUrl(fileHash)}
      fallback="empty"
      alt={alt}
      loading={loading}
      draggable={draggable}
      className={className}
      onThumbnailError={onError}
      style={{
        position: 'absolute',
        inset: 0,
        display: 'block',
        width: '100%',
        height: '100%',
        objectFit: 'cover',
        userSelect: 'none',
        pointerEvents: 'none',
      }}
    />
  );
}

export function SubscriptionCoverDisplay({
  fileHash,
  crop,
  ...props
}: {
  fileHash: string;
  crop: SubscriptionCoverCrop;
  className?: string;
  alt?: string;
  loading?: 'eager' | 'lazy';
  draggable?: boolean;
  onError?: () => void;
}) {
  const fixedThumbnail = crop.focusX === 500
    && crop.focusY === 500
    && crop.zoomPercent === 100;
  if (fixedThumbnail) {
    return <SubscriptionCoverThumbnail fileHash={fileHash} {...props} />;
  }
  return <SubscriptionCoverImage fileHash={fileHash} crop={crop} {...props} />;
}

export function subscriptionCoverGeometry(
  dimensions: SubscriptionCoverDimensions,
  crop: SubscriptionCoverCrop,
) {
  const width = Math.max(1, dimensions.width);
  const height = Math.max(1, dimensions.height);
  const aspect = width / height;
  const zoom = crop.zoomPercent / 100;
  const widthRatio = Math.max(1, aspect) * zoom;
  const heightRatio = Math.max(1, 1 / aspect) * zoom;
  const focusX = crop.focusX / 1000;
  const focusY = crop.focusY / 1000;
  return {
    widthRatio,
    heightRatio,
    leftPercent: 50 + (0.5 - focusX) * (widthRatio - 1) * 100,
    topPercent: 50 + (0.5 - focusY) * (heightRatio - 1) * 100,
  };
}

export function SubscriptionCoverImage({
  fileHash,
  thumbnailLibraryPath,
  crop,
  fallbackDimensions,
  className,
  alt = '',
  loading,
  draggable = false,
  onDimensionsChange,
  onError,
  preferThumbnail = false,
  progressive = false,
}: {
  fileHash: string;
  thumbnailLibraryPath?: string | null;
  crop: SubscriptionCoverCrop;
  fallbackDimensions?: SubscriptionCoverDimensions;
  className?: string;
  alt?: string;
  loading?: 'eager' | 'lazy';
  draggable?: boolean;
  onDimensionsChange?: (dimensions: SubscriptionCoverDimensions) => void;
  onError?: () => void;
  preferThumbnail?: boolean;
  progressive?: boolean;
}) {
  const [dimensions, setDimensions] = useState(fallbackDimensions ?? { width: 1, height: 1 });
  const [useThumbnail, setUseThumbnail] = useState(preferThumbnail);
  const [originalReady, setOriginalReady] = useState(false);
  const thumbnailUrl = mediaThumbnailUrl(fileHash, thumbnailLibraryPath);

  useEffect(() => {
    setUseThumbnail(preferThumbnail);
    setOriginalReady(false);
    setDimensions(fallbackDimensions ?? { width: 1, height: 1 });
  }, [fallbackDimensions?.height, fallbackDimensions?.width, fileHash, preferThumbnail, thumbnailLibraryPath]);

  const geometry = subscriptionCoverGeometry(dimensions, crop);
  const imageStyle = {
    position: 'absolute' as const,
    display: 'block',
    maxWidth: 'none',
    width: `${geometry.widthRatio * 100}%`,
    height: `${geometry.heightRatio * 100}%`,
    left: `${geometry.leftPercent}%`,
    top: `${geometry.topPercent}%`,
    objectFit: 'fill' as const,
    transform: 'translate(-50%, -50%)',
    userSelect: 'none' as const,
    pointerEvents: 'none' as const,
  };

  const original = (
    <ThumbnailImage
      src={useThumbnail
        ? thumbnailUrl
        : mediaFileUrl(fileHash, 'application/octet-stream')}
      fallback="empty"
      alt={alt}
      loading={loading}
      draggable={draggable}
      className={className}
      onLoad={(event) => {
        const next = {
          width: event.currentTarget.naturalWidth,
          height: event.currentTarget.naturalHeight,
        };
        setDimensions(next);
        setOriginalReady(true);
        onDimensionsChange?.(next);
      }}
      onThumbnailError={() => {
        if (!useThumbnail) setUseThumbnail(true);
        else onError?.();
      }}
      style={progressive && !useThumbnail ? {
        ...imageStyle,
        opacity: originalReady ? 1 : 0,
        transition: 'opacity 160ms ease-out',
      } : imageStyle}
    />
  );

  if (!progressive || useThumbnail) return original;

  return (
    <>
      <ThumbnailImage
        src={thumbnailUrl}
        fallback="empty"
        alt=""
        draggable={draggable}
        className={className}
        style={imageStyle}
      />
      {original}
    </>
  );
}
