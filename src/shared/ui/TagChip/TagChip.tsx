/**
 * TagChip — shared label item with namespace coloring and optional icon.
 * Used for both tags and folders in the inspector.
 */

import type { ReactNode } from 'react';
import { InspectorRemoveIcon } from '../icons/toolbar-icons';
import { KbdTooltip } from '../KbdTooltip';
import styles from './TagChip.module.css';

const NS_COLORS: Record<string, [number, number, number]> = {
  creator: [255, 102, 103],
  studio: [255, 102, 103],
  character: [48, 209, 89],
  person: [48, 209, 89],
  series: [196, 153, 255],
  species: [0, 170, 255],
  rating: [255, 214, 10],
  meta: [189, 190, 192],
  system: [255, 159, 10],
  '': [189, 190, 192],
  default: [189, 190, 192],
};

const TAG_TEXT_COLORS = {
  neutral: ['var(--inspector-text-primary, var(--color-text-primary))', 'var(--inspector-text-primary, var(--color-text-primary))'],
  red: ['#F8E7E6', '#513636'],
  orange: ['#F8F0E1', '#4C3E31'],
  yellow: ['#F8F5E1', '#4A4335'],
  green: ['#E4F5E9', '#34403C'],
  aqua: ['#E5F4F6', '#234449'],
  blue: ['#DFF1F9', '#3C4E5A'],
  purple: ['#F3EFF9', '#3F3B4A'],
  pink: ['#F8EFF4', '#584753'],
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
  showNamespace?: boolean;
  onRemove?: () => void;
  onClick?: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
}

export function TagChip({ namespace, subtag, icon, colorRgb, showNamespace = true, onRemove, onClick, onContextMenu }: Props) {
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

  const namespaceVisible = showNamespace && namespace !== 'default' && namespace !== '' && namespace !== 'general';

  return (
    <span
      className={`${styles.chip} ${onRemove ? styles.chipWithRemove : ''}`}
      style={chipStyle}
      onClick={onClick}
      onContextMenu={onContextMenu}
    >
      {icon && <span className={styles.iconSlot}>{icon}</span>}
      {namespaceVisible && <span className={styles.namespace}>{namespace}:</span>}
      <span className={styles.subtag}>{subtag}</span>
      {onRemove && (
        <KbdTooltip label="Remove">
          <button
            className={styles.removeBtn}
            onClick={(e) => { e.stopPropagation(); onRemove(); }}
            type="button"
            aria-label="Remove"
          >
            <InspectorRemoveIcon />
          </button>
        </KbdTooltip>
      )}
    </span>
  );
}
