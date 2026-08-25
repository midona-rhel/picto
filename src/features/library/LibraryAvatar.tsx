import { useEffect, useState } from 'react';
import { IconBooks } from '@tabler/icons-react';
import { mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import styles from './LibraryAvatar.module.css';

export interface LibraryAppearance {
  icon?: string | null;
  color?: string | null;
  imageHash?: string | null;
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
        <img
          className={styles.image}
          src={mediaThumbnailUrl(imageHash)}
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
