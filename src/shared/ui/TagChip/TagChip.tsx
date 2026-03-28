/**
 * TagChip — namespace-colored tag pill with optional remove button.
 * Stateless and presentational.
 */

import { IconX } from '@tabler/icons-react';
import styles from './TagChip.module.css';

const NS_COLORS: Record<string, [number, number, number]> = {
  creator: [170, 0, 0],
  studio: [128, 0, 0],
  character: [0, 170, 0],
  person: [0, 128, 0],
  series: [170, 0, 170],
  species: [0, 130, 170],
  meta: [160, 160, 160],
  system: [153, 101, 21],
  '': [114, 160, 193],
  default: [114, 160, 193],
};

interface Props {
  namespace: string;
  subtag: string;
  onRemove?: () => void;
}

export function TagChip({ namespace, subtag, onRemove }: Props) {
  const [r, g, b] = NS_COLORS[namespace.toLowerCase()] ?? NS_COLORS.default;
  const style = {
    background: `rgba(${r}, ${g}, ${b}, 0.12)`,
    border: `1px solid rgba(${r}, ${g}, ${b}, 0.25)`,
  };

  const showNamespace = namespace !== 'default' && namespace !== '';

  return (
    <span className={styles.chip} style={style}>
      {showNamespace && <span className={styles.namespace}>{namespace}:</span>}
      <span className={styles.subtag}>{subtag}</span>
      {onRemove && (
        <button className={styles.removeBtn} onClick={onRemove} type="button" title="Remove tag">
          <IconX size={10} stroke={2} />
        </button>
      )}
    </span>
  );
}
