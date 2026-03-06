import { useMemo, memo } from 'react';
import { Badge } from '@mantine/core';
import { IconPhoto } from '@tabler/icons-react';
import { MasonryImageItem } from './shared';
import { getCachedMediaUrl } from './enhancedMediaCache';
import previewStyles from './GlassImagePreview.module.css';
import {
  MediaCardFrame,
  MediaCardImage,
} from '../ui/media-card';

const CONTAINER_HEIGHT = 200;

interface GlassImagePreviewProps {
  images: MasonryImageItem[];
}

export const GlassImagePreview = memo(function GlassImagePreview({ images }: GlassImagePreviewProps) {
  // Build URLs synchronously during render — no useEffect delay
  const thumbUrls = useMemo(() => {
    const urls = new Map<string, string>();
    for (const img of images) {
      const url = getCachedMediaUrl(img.hash, 'thumb512');
      if (url) urls.set(img.hash, url);
    }
    return urls;
  }, [images]);

  if (images.length === 0) {
    return (
      <div className={previewStyles.container} style={{ height: CONTAINER_HEIGHT }}>
        <MediaCardFrame className={previewStyles.imageBox}>
          <MediaCardImage
            fit="contain"
            fallback={<IconPhoto size={48} className={previewStyles.placeholderIcon} />}
          />
        </MediaCardFrame>
      </div>
    );
  }

  // Single image — same as original behavior
  if (images.length === 1) {
    const image = images[0];
    const thumbUrl = thumbUrls.get(image.hash) || '';
    const mimeLabel = image.mime?.split('/').pop()?.toUpperCase() || 'IMG';

    return (
      <div className={previewStyles.container} style={{ height: CONTAINER_HEIGHT }}>
        {thumbUrl ? (
          <MediaCardFrame className={previewStyles.singleImage}>
            <MediaCardImage
              src={thumbUrl}
              fit="contain"
              imageClassName={previewStyles.singleImageTag}
            />
            <Badge size="xs" variant="filled" color="dark" className={previewStyles.mimeBadge}>
              {mimeLabel}
            </Badge>
          </MediaCardFrame>
        ) : (
          <MediaCardFrame className={previewStyles.imageBox}>
            <MediaCardImage
              fit="contain"
              fallback={<IconPhoto size={48} className={previewStyles.placeholderIcon} />}
            />
          </MediaCardFrame>
        )}
      </div>
    );
  }

  // Multi-image stacked preview — show last 3 thumbnails
  const stackImages = images.slice(-3);
  const rotations = [-4, 2, 0];
  const offsets = [{ x: -8, y: 4 }, { x: 6, y: -3 }, { x: 0, y: 0 }];
  // Adjust for fewer than 3
  const startIdx = 3 - stackImages.length;

  return (
    <div className={previewStyles.container} style={{ height: CONTAINER_HEIGHT }}>
      <div className={previewStyles.stackContainer}>
        {stackImages.map((img, i) => {
          const idx = startIdx + i;
          const url = thumbUrls.get(img.hash) || '';
          const rot = rotations[idx];
          const off = offsets[idx];
          const isTop = i === stackImages.length - 1;

          return (
            <div
              key={img.hash}
              className={previewStyles.stackItem}
              style={{
                transform: `rotate(${rot}deg) translate(${off.x}px, ${off.y}px)`,
                zIndex: i,
                filter: isTop ? undefined : 'brightness(0.7)',
              }}
            >
              {url ? (
                <MediaCardFrame className={previewStyles.stackCard}>
                  <MediaCardImage
                    src={url}
                    fit="contain"
                    imageClassName={previewStyles.stackImage}
                  />
                </MediaCardFrame>
              ) : (
                <MediaCardFrame className={previewStyles.stackPlaceholder}>
                  <MediaCardImage
                    fit="contain"
                    fallback={<IconPhoto size={32} className={previewStyles.stackPlaceholderIcon} />}
                  />
                </MediaCardFrame>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
});
