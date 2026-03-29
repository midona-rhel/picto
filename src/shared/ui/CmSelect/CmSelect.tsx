/**
 * CmSelect — custom dropdown select for use inside context menus / portals.
 * No MantineProvider dependency. Matches legacy cmSelectInput/Dropdown styling.
 */

import { useState, useRef, useEffect } from 'react';
import { IconSelector } from '@tabler/icons-react';
import styles from './CmSelect.module.css';

export interface CmSelectOption {
  value: string;
  label: string;
  icon?: React.ReactNode;
}

interface Props {
  value: string;
  options: CmSelectOption[];
  onChange: (value: string) => void;
  /** Fixed width in px. Defaults to auto. */
  width?: number;
}

export function CmSelect({ value, options, onChange, width }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const cur = options.find((o) => o.value === value);
  const hasIcons = options.some((o) => o.icon);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  return (
    <div ref={ref} className={styles.root}>
      <button
        className={`${styles.btn} ${cur?.icon ? styles.btnWithIcon : ''}`}
        style={width ? { width } : undefined}
        onClick={(e) => { e.stopPropagation(); setOpen(!open); }}
        type="button"
      >
        {cur?.icon && <span className={styles.btnIcon}>{cur.icon}</span>}
        <span className={styles.btnLabel}>{cur?.label ?? value}</span>
        <span className={styles.btnChevron}><IconSelector size={14} /></span>
      </button>
      {open && (
        <div className={styles.drop}>
          {options.map((o) => (
            <button
              key={o.value}
              className={`${styles.opt} ${o.value === value ? styles.optActive : ''}`}
              onClick={(e) => { e.stopPropagation(); onChange(o.value); setOpen(false); }}
              type="button"
            >
              {hasIcons && <span className={styles.optIcon}>{o.icon ?? null}</span>}
              <span>{o.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
