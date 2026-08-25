import { useEffect, useState } from 'react';
import { IconBooks } from '@tabler/icons-react';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { SubscriptionCoverImage } from '../subscriptions/components/SubscriptionCoverImage';
import styles from './LibraryAvatar.module.css';

export interface LibraryAppearance {
  icon?: string | null;
  color?: string | null;
  imageHash?: string | null;
  imageFocusX?: number | null;
  imageFocusY?: number | null;
  imageZoomPercent?: number | null;
}

export function LibraryAvatar({
  appearance,
  size,
  className,
}: {
  appearance: LibraryAppearance;
  size: number;
  className?: string;
}) {
  const [imageFailed, setImageFailed] = useState(false);
  const imageHash = appearance.imageHash ?? null;

  useEffect(() => setImageFailed(false), [imageHash]);

  return (
    <span
      className={`${styles.avatar}${className ? ` ${className}` : ''}`}
      style={{ width: size, height: size, color: appearance.color ?? undefined }}
    >
      {imageHash && !imageFailed ? (
        <SubscriptionCoverImage
          className={styles.image}
          fileHash={imageHash}
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
        <DynamicIcon name={appearance.icon} size={size} color={appearance.color ?? null} />
      ) : (
        <IconBooks size={size} stroke={1} />
      )}
    </span>
  );
}
