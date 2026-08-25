import { IconEqual, IconLayersIntersect, IconLayersUnion } from '@tabler/icons-react';
import type { FilterMatchMode } from '../../types/generated/application/FilterMatchMode';
import styles from './FilterLogicTabs.module.css';

const OPTIONS = [
  { value: 'any', label: 'Match any', icon: IconLayersUnion },
  { value: 'all', label: 'Match all', icon: IconLayersIntersect },
  { value: 'exact', label: 'Match exactly', icon: IconEqual },
] as const;

export function FilterLogicTabs({ value, onChange }: {
  value: FilterMatchMode;
  onChange: (value: FilterMatchMode) => void;
}) {
  return (
    <div className={styles.root} role="group" aria-label="Filter matching rule">
      {OPTIONS.map((option) => {
        const Icon = option.icon;
        return (
          <button
            key={option.value}
            type="button"
            className={`${styles.button} ${value === option.value ? styles.active : ''}`}
            aria-label={option.label}
            aria-pressed={value === option.value}
            title={option.label}
            onClick={() => onChange(option.value)}
          >
            <Icon size={14} stroke={1.7} />
          </button>
        );
      })}
    </div>
  );
}
