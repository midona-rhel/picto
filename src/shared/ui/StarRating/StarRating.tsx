import { useState } from 'react';
import { IconStar, IconStarFilled } from '@tabler/icons-react';
import { PropertyRow } from '../PropertyRow/PropertyRow';
import styles from './StarRating.module.css';
import { t } from '../../../i18n';

interface Props {
  value: number;
  label?: string;
  onChange?: (star: number) => void | Promise<void>;
  onError?: (reason: unknown) => void;
}

export function StarRating({ value, label, onChange, onError }: Props) {
  const [hovered, setHovered] = useState(0);
  const [pending, setPending] = useState(false);
  const interactive = !!onChange && !pending;
  const display = hovered > 0 ? hovered : value;

  const changeRating = (star: number) => {
    if (!onChange || pending) return;
    const next = star === value ? 0 : star;
    try {
      const result = onChange(next);
      if (result && typeof result.then === 'function') {
        setPending(true);
        void result
          .catch((reason) => onError?.(reason))
          .finally(() => setPending(false));
      }
    } catch (reason) {
      onError?.(reason);
    }
  };

  return (
    <PropertyRow
      label={label ?? t('Rating')}
      value={null}
      content={(
      <div className={styles.stars} onMouseLeave={() => setHovered(0)}>
        {[1, 2, 3, 4, 5].map((star) => (
          <button
            key={star}
            className={`${styles.star} ${star <= display ? styles.active : styles.inactive} ${!interactive ? styles.disabled : ''}`}
            onClick={() => changeRating(star)}
            onMouseEnter={() => interactive && setHovered(star)}
            disabled={!interactive}
            type="button"
            aria-label={star === value ? t("Clear rating") : t("Set rating to {value0} star{value1}", { value0: star, value1: star === 1 ? '' : 's' })}
            aria-pressed={star <= value}
          >
            {star <= display
              ? <IconStarFilled size={12} />
              : <IconStar size={12} />}
          </button>
        ))}
      </div>
      )}
    />
  );
}
