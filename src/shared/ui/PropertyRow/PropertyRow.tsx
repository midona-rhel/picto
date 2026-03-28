import styles from './PropertyRow.module.css';

interface Props {
  label: string;
  value: string | null | undefined;
  mono?: boolean;
  title?: string;
}

export function PropertyRow({ label, value, mono, title }: Props) {
  if (value == null) return null;
  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <span className={mono ? styles.valueMono : styles.value} title={title}>{value}</span>
    </div>
  );
}
