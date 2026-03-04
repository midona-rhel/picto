import type { ReactNode } from 'react';
import styles from './PropertyRow.module.css';

interface PropertyRowProps {
  label: string;
  value?: string | number;
  mono?: boolean;
  title?: string;
  children?: ReactNode;
}

export function PropertyRow({ label, value, mono, title, children }: PropertyRowProps) {
  return (
    <div className={styles.propRow}>
      <span className={styles.propLabel}>{label}</span>
      {children ? (
        <span className={mono ? styles.propValueMono : styles.propValue} title={title}>
          {children}
        </span>
      ) : (
        <span className={mono ? styles.propValueMono : styles.propValue} title={title}>
          {value}
        </span>
      )}
    </div>
  );
}
