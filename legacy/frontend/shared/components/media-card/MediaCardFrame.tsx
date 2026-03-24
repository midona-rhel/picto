import type { ReactNode } from 'react';
import styles from './MediaCard.module.css';

interface MediaCardFrameProps {
  children: ReactNode;
  className?: string;
}

export function MediaCardFrame({ children, className }: MediaCardFrameProps) {
  return <div className={[styles.frame, className].filter(Boolean).join(' ')}>{children}</div>;
}
