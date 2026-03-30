import styles from '../SubscriptionsScreen.module.css';

export function EmptyState({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div className={styles.emptyState}>
      <div className={styles.sectionTitle}>{title}</div>
      <div className={styles.muted}>{description}</div>
    </div>
  );
}
