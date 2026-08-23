import styles from './PropertyRow.module.css';

interface Props {
  label: string;
  value: string | null | undefined;
  mono?: boolean;
  title?: string;
  loading?: boolean;
  showLoading?: boolean;
}

export function PropertyRow({ label, value, mono, title, loading = false, showLoading = false }: Props) {
  if (value == null && !loading) return null;
  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <span className={mono ? styles.valueMono : styles.value} title={title}>
        {loading
          ? showLoading && <span className={styles.spinner} data-inspector-summary-loading="" aria-label={`Loading ${label.toLowerCase()}`} />
          : value}
      </span>
    </div>
  );
}
