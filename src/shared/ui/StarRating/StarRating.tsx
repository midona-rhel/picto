import { IconStar, IconStarFilled } from '@tabler/icons-react';
import styles from './StarRating.module.css';

interface Props {
  value: number;
  label?: string;
  onChange?: (star: number) => void;
}

export function StarRating({ value, label = 'Rating', onChange }: Props) {
  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <div className={styles.stars}>
        {[1, 2, 3, 4, 5].map((star) => (
          <button
            key={star}
            className={`${styles.star} ${star <= value ? styles.active : styles.inactive} ${!onChange ? styles.disabled : ''}`}
            onClick={() => onChange?.(star === value ? 0 : star)}
            disabled={!onChange}
            type="button"
          >
            {star <= value
              ? <IconStarFilled size={12} />
              : <IconStar size={12} />}
          </button>
        ))}
      </div>
    </div>
  );
}
