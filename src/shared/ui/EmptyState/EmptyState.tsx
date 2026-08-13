import type { ButtonHTMLAttributes, ReactNode } from 'react';
import styles from './EmptyState.module.css';

interface EmptyStateProps {
  icon: ReactNode;
  title: string;
  description: string;
  actions?: ReactNode;
  progress?: ReactNode;
}

export function EmptyState({ icon, title, description, actions, progress }: EmptyStateProps) {
  return (
    <section className={styles.root} aria-label={title}>
      <div className={styles.frame} aria-hidden="true">
        <div className={styles.frameGlass}>
          <div className={styles.frameInner}>{icon}</div>
        </div>
      </div>
      <span className={styles.title}>{title}</span>
      <span className={styles.description}>{description}</span>
      {actions && <div className={styles.actions}>{actions}</div>}
      {progress !== undefined && <div className={styles.progress}>{progress}</div>}
    </section>
  );
}

export function EmptyStateAction({ className, ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button {...props} type={props.type ?? 'button'} className={`${styles.action} ${className ?? ''}`} />;
}
