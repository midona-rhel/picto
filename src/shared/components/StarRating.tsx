import { useState, useCallback } from 'react';
import { IconStar, IconStarFilled } from '@tabler/icons-react';
import styles from './StarRating.module.css';

interface StarRatingProps {
  value: number;
  onChange?: (rating: number) => void;
  label?: string;
  size?: number;
}

export function StarRating({ value, onChange, label = 'Rating', size = 14 }: StarRatingProps) {
  const [hoveredStar, setHoveredStar] = useState<number | null>(null);
  const readOnly = !onChange;
  const displayValue = hoveredStar ?? value;

  const handleClick = useCallback((star: number) => {
    if (!onChange) return;
    onChange(star === value ? 0 : star);
  }, [onChange, value]);

  return (
    <div className={styles.ratingRow}>
      <span style={{ color: 'var(--color-text-secondary)', fontSize: 11, minWidth: 64, overflow: 'hidden' }}>
        {label}
      </span>
      <div
        className={styles.ratingStars}
        onMouseLeave={readOnly ? undefined : () => setHoveredStar(null)}
      >
        {[1, 2, 3, 4, 5].map((star) => (
          <button
            key={star}
            className={`${styles.ratingStar} ${displayValue >= star ? styles.ratingStarActive : styles.ratingStarInactive} ${readOnly ? styles.ratingStarDisabled : ''}`}
            onClick={() => handleClick(star)}
            onMouseEnter={readOnly ? undefined : () => setHoveredStar(star)}
            disabled={readOnly}
          >
            {displayValue >= star ? (
              <IconStarFilled size={size} />
            ) : (
              <IconStar size={size} />
            )}
          </button>
        ))}
      </div>
    </div>
  );
}
