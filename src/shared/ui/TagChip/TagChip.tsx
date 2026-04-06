/**
 * TagChip — reference application-matched label-item with namespace coloring and optional icon.
 * Used for both tags and folders in the inspector.
 */

import type { ReactNode } from 'react';
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
  icon?: ReactNode;
  colorRgb?: [number, number, number];
  onRemove?: () => void;
  onClick?: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
}

export function TagChip({ namespace, subtag, icon, colorRgb, onRemove, onClick, onContextMenu }: Props) {
  const [r, g, b] = colorRgb ?? NS_COLORS[(namespace ?? '').toLowerCase()] ?? NS_COLORS.default;
  const chipStyle = {
    '--chip-bg': `rgba(${r}, ${g}, ${b}, 0.10)`,
    '--chip-border': `rgba(${r}, ${g}, ${b}, 0.25)`,
    '--chip-hover-bg': `rgba(${r}, ${g}, ${b}, 0.20)`,
    '--chip-hover-border': `rgba(${r}, ${g}, ${b}, 0.50)`,
  } as React.CSSProperties;

  const showNamespace = namespace !== 'default' && namespace !== '' && namespace !== 'general';

  return (
    <span
      className={`${styles.chip} ${onRemove ? styles.chipWithRemove : ''}`}
      style={chipStyle}
      onClick={onClick}
      onContextMenu={onContextMenu}
    >
      {icon && <span className={styles.iconSlot}>{icon}</span>}
      {showNamespace && <span className={styles.namespace}>{namespace}:</span>}
      <span className={styles.subtag}>{subtag}</span>
      {onRemove && (
        <button
          className={styles.removeBtn}
          onClick={(e) => { e.stopPropagation(); onRemove(); }}
          type="button"
          title="Remove"
        >
          <IconX size={10} stroke={2} />
        </button>
      )}
    </span>
  );
}
