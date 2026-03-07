import type { ReactNode } from 'react';
import styles from './MediaCard.module.css';

interface MediaCardMetaProps {
  children: ReactNode;
  className?: string;
}

export function MediaCardMeta({ children, className }: MediaCardMetaProps) {
  return <div className={[styles.meta, className].filter(Boolean).join(' ')}>{children}</div>;
}
