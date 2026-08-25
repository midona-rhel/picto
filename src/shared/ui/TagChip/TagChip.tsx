/**
 * TagChip — shared label item with namespace coloring and optional icon.
 * Used for both tags and folders in the inspector.
 */

import type { ReactNode } from 'react';
import { InspectorRemoveIcon } from '../icons/toolbar-icons';
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

const TAG_TEXT_COLORS = {
  neutral: ['var(--inspector-text-primary, var(--color-text-primary))', 'var(--inspector-text-primary, var(--color-text-primary))'],
  red: ['#F8E6E5', '#88403E'],
  orange: ['#F8EFE1', '#7C5435'],
  yellow: ['#F8F6E1', '#77623E'],
  green: ['#E3F5E8', '#40594B'],
  aqua: ['#E5F3F6', '#16636C'],
  blue: ['#DEF0F8', '#345A78'],
  purple: ['#F2EEF9', '#5C496E'],
  pink: ['#F8EEF4', '#7D4A66'],
} as const;

function tagTextColors([r, g, b]: [number, number, number]): readonly [string, string] {
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const delta = max - min;
  if (delta < 20) return TAG_TEXT_COLORS.neutral;

  let hue: number;
  if (max === r) hue = 60 * (((g - b) / delta) % 6);
  else if (max === g) hue = 60 * ((b - r) / delta + 2);
  else hue = 60 * ((r - g) / delta + 4);
  if (hue < 0) hue += 360;

  if (hue < 15 || hue >= 345) return TAG_TEXT_COLORS.red;
  if (hue < 45) return TAG_TEXT_COLORS.orange;
  if (hue < 75) return TAG_TEXT_COLORS.yellow;
  if (hue < 155) return TAG_TEXT_COLORS.green;
  if (hue < 195) return TAG_TEXT_COLORS.aqua;
  if (hue < 245) return TAG_TEXT_COLORS.blue;
  if (hue < 285) return TAG_TEXT_COLORS.purple;
  return TAG_TEXT_COLORS.pink;
}

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
  const [darkText, lightText] = tagTextColors([r, g, b]);
  const chipStyle = {
    '--chip-bg': `rgba(${r}, ${g}, ${b}, 0.10)`,
    '--chip-border': `rgba(${r}, ${g}, ${b}, 0.25)`,
    '--chip-hover-bg': `rgba(${r}, ${g}, ${b}, 0.20)`,
    '--chip-hover-border': `rgba(${r}, ${g}, ${b}, 0.50)`,
    '--chip-text-dark': darkText,
    '--chip-text-light': lightText,
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
          <InspectorRemoveIcon />
        </button>
      )}
    </span>
  );
}
