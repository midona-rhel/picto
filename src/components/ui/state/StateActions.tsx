import type { ReactNode } from 'react';
import styles from './StateBlock.module.css';

interface StateActionsProps {
  children: ReactNode;
}

export function StateActions({ children }: StateActionsProps) {
  return <div className={styles.actions}>{children}</div>;
}
