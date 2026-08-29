import { useEffect, useState } from 'react';
import { IconBooks } from '@tabler/icons-react';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { libraryCoverUrl } from '../../shared/lib/mediaUrl';
import { SubscriptionCoverImage } from '../subscriptions/components/SubscriptionCoverImage';
import styles from './LibraryAvatar.module.css';

export interface LibraryAppearance {
  icon?: string | null;
  color?: string | null;
  imageHash?: string | null;
  imageFocusX?: number | null;
  imageFocusY?: number | null;
  imageZoomPercent?: number | null;
  libraryPath?: string | null;
}

export function LibraryAvatar({
  appearance,
  size,
  className,
  highlighted = false,
}: {
  appearance: LibraryAppearance;
  size: number;
  className?: string;
  highlighted?: boolean;
}) {
  const [imageFailed, setImageFailed] = useState(false);
  const imageHash = appearance.imageHash ?? null;

  useEffect(() => setImageFailed(false), [appearance.libraryPath, imageHash]);

  return (
    <span
      className={`${styles.avatar}${highlighted ? ` ${styles.highlighted}` : ''}${className ? ` ${className}` : ''}`}
      style={{ width: size, height: size, color: appearance.color ?? undefined }}
    >
      {imageHash && !imageFailed ? (
        <SubscriptionCoverImage
          className={styles.image}
          fileHash={imageHash}
          thumbnailUrlOverride={appearance.libraryPath ? libraryCoverUrl(appearance.libraryPath) : undefined}
          crop={{
            focusX: appearance.imageFocusX ?? 500,
            focusY: appearance.imageFocusY ?? 500,
            zoomPercent: appearance.imageZoomPercent ?? 100,
          }}
          preferThumbnail
          alt=""
          draggable={false}
          onError={() => setImageFailed(true)}
        />
      ) : appearance.icon ? (
        <DynamicIcon name={appearance.icon} size={Math.round(size * 0.72)} color={appearance.color ?? null} />
      ) : (
        <IconBooks size={Math.round(size * 0.72)} stroke={0.9} />
      )}
    </span>
  );
}
