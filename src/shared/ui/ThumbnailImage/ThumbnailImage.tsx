import { useEffect, useState, type ImgHTMLAttributes } from 'react';
import { subscribeThumbnailChanged } from '../../lib/thumbnailChanges';
import { BrokenThumbnail } from './BrokenThumbnail';
import { FontThumbnail } from './FontThumbnail';

type ThumbnailImageProps = Omit<ImgHTMLAttributes<HTMLImageElement>, 'onError'> & {
  fallback?: 'broken' | 'font' | 'empty';
  onThumbnailError?: () => void;
};

/** One thumbnail failure boundary for DOM surfaces. Chromium's native broken
 * image glyph must never leak into Picto's UI. */
export function ThumbnailImage({
  src,
  fallback = 'broken',
  onThumbnailError,
  ...imageProps
}: ThumbnailImageProps) {
  const [failed, setFailed] = useState(false);
  const [revision, setRevision] = useState(0);

  useEffect(() => setFailed(false), [src]);
  useEffect(() => subscribeThumbnailChanged((fileHash) => {
    if (typeof src === 'string' && src.includes(`/thumb/${fileHash}.jpg`)) {
      setFailed(false);
      setRevision((current) => current + 1);
    }
  }), [src]);

  if (fallback === 'font') {
    return <FontThumbnail className={imageProps.className} style={imageProps.style} />;
  }

  if (failed) {
    if (fallback === 'empty') return null;
    return <BrokenThumbnail className={imageProps.className} style={imageProps.style} />;
  }

  return (
    <img
      {...imageProps}
      src={typeof src === 'string' && revision > 0
        ? `${src}${src.includes('?') ? '&' : '?'}revision=${revision}`
        : src}
      onError={() => {
        setFailed(true);
        onThumbnailError?.();
      }}
    />
  );
}
