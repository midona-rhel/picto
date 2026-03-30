import type { ReactNode } from 'react';
import styles from './WorkspaceSwitcher.module.css';

export type WorkspaceOption<T extends string> = {
  value: T;
  label: string;
  icon?: ReactNode;
};

export function WorkspaceSwitcher<T extends string>({
  options,
  value,
  onChange,
}: {
  options: WorkspaceOption<T>[];
  value: T;
  onChange: (next: T) => void;
}) {
  return (
    <div className={styles.root}>
      {options.map((option) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            className={`${styles.button} ${active ? styles.buttonActive : ''}`.trim()}
            onClick={() => onChange(option.value)}
          >
            {option.icon}
            <span>{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
