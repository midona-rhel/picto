import { useState } from 'react';
import { IconStar, IconStarFilled } from '@tabler/icons-react';
import styles from './StarRating.module.css';

interface Props {
  value: number;
  label?: string;
  onChange?: (star: number) => void;
}

export function StarRating({ value, label = 'Rating', onChange }: Props) {
  const [hovered, setHovered] = useState(0);
  const interactive = !!onChange;
  const display = hovered > 0 ? hovered : value;

  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <div className={styles.stars} onMouseLeave={() => setHovered(0)}>
        {[1, 2, 3, 4, 5].map((star) => (
          <button
            key={star}
            className={`${styles.star} ${star <= display ? styles.active : styles.inactive} ${!interactive ? styles.disabled : ''}`}
            onClick={() => onChange?.(star === value ? 0 : star)}
            onMouseEnter={() => interactive && setHovered(star)}
            disabled={!interactive}
            type="button"
          >
            {star <= display
              ? <IconStarFilled size={12} />
              : <IconStar size={12} />}
          </button>
        ))}
      </div>
    </div>
  );
}
