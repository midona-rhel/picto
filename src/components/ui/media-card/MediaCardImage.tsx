import type { ReactNode } from 'react';
import styles from './MediaCard.module.css';

interface MediaCardImageProps {
  src?: string | null;
  alt?: string;
  fit?: 'cover' | 'contain';
  className?: string;
  imageClassName?: string;
  fallback?: ReactNode;
  loading?: 'eager' | 'lazy';
}

export function MediaCardImage({
  src,
  alt = '',
  fit = 'cover',
  className,
  imageClassName,
  fallback,
  loading,
}: MediaCardImageProps) {
  return (
    <div className={[styles.imageRoot, className].filter(Boolean).join(' ')}>
      {src ? (
        <img
          src={src}
          alt={alt}
          loading={loading}
          className={[
            styles.image,
            fit === 'contain' ? styles.contain : styles.cover,
            imageClassName,
          ].filter(Boolean).join(' ')}
        />
      ) : (
        <div className={styles.fallback}>{fallback}</div>
      )}
    </div>
  );
}
