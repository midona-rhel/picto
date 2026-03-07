import type { ReactNode } from 'react';
import styles from './MediaCard.module.css';

interface MediaCardOverlayProps {
  children: ReactNode;
  position?: 'top' | 'bottom';
  tone?: 'strong' | 'soft' | 'none';
  className?: string;
  contentClassName?: string;
}

export function MediaCardOverlay({
  children,
  position = 'bottom',
  tone = 'strong',
  className,
  contentClassName,
}: MediaCardOverlayProps) {
  return (
    <div
      className={[
        styles.overlay,
        position === 'top' ? styles.overlayTop : styles.overlayBottom,
        tone === 'strong' ? styles.overlayGradientStrong : '',
        tone === 'soft' ? styles.overlayGradientSoft : '',
        className,
      ].filter(Boolean).join(' ')}
    >
      <div className={[styles.overlayContent, contentClassName].filter(Boolean).join(' ')}>
        {children}
      </div>
    </div>
  );
}
