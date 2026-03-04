import { useMemo, memo } from 'react';
import { Badge } from '@mantine/core';
import { IconPhoto } from '@tabler/icons-react';
import { MasonryImageItem } from './shared';
import { getCachedMediaUrl } from './enhancedMediaCache';
import previewStyles from './GlassImagePreview.module.css';

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
        <div className={previewStyles.imageBox}>
          <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
            <IconPhoto size={48} style={{ opacity: 0.3 }} />
          </div>
        </div>
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
          <div className={previewStyles.singleImage}>
            <img src={thumbUrl} alt="" style={{ display: 'block', maxWidth: '100%', maxHeight: CONTAINER_HEIGHT, objectFit: 'contain' }} />
            <Badge size="xs" variant="filled" color="dark" className={previewStyles.mimeBadge}>
              {mimeLabel}
            </Badge>
          </div>
        ) : (
          <div className={previewStyles.imageBox}>
            <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <IconPhoto size={48} style={{ opacity: 0.3 }} />
            </div>
          </div>
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
              style={{
                position: 'absolute',
                inset: 0,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                transform: `rotate(${rot}deg) translate(${off.x}px, ${off.y}px)`,
                zIndex: i,
                filter: isTop ? undefined : 'brightness(0.7)',
              }}
            >
              {url ? (
                <div className={previewStyles.stackCard}>
                  <img src={url} alt="" style={{ display: 'block', maxWidth: '100%', maxHeight: CONTAINER_HEIGHT - 20, objectFit: 'contain' }} />
                </div>
              ) : (
                <div className={previewStyles.stackPlaceholder}>
                  <IconPhoto size={32} style={{ opacity: 0.2 }} />
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
});
