import { IconEqual, IconLayersIntersect, IconLayersUnion } from '@tabler/icons-react';
import type { SetMatchMode } from '../../types/canonical';
import styles from './FilterLogicTabs.module.css';
import { KbdTooltip } from '../KbdTooltip';

const OPTIONS = [
  { value: 'any', label: 'Match any', icon: IconLayersUnion },
  { value: 'all', label: 'Match all', icon: IconLayersIntersect },
  { value: 'exact', label: 'Match exactly', icon: IconEqual },
] as const;

export function FilterLogicTabs({ value, onChange }: {
  value: SetMatchMode;
  onChange: (value: SetMatchMode) => void;
}) {
  return (
    <div className={styles.root} role="group" aria-label="Filter matching rule">
      {OPTIONS.map((option) => {
        const Icon = option.icon;
        return (
          <KbdTooltip key={option.value} label={option.label}>
          <button
            type="button"
            className={`${styles.button} ${value === option.value ? styles.active : ''}`}
            aria-label={option.label}
            aria-pressed={value === option.value}
            onClick={() => onChange(option.value)}
          >
            <Icon size={14} stroke={1.7} />
          </button>
          </KbdTooltip>
        );
      })}
    </div>
  );
}
