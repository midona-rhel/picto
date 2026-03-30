/**
 * CmSelect — custom dropdown select for use anywhere (portals, scroll containers).
 * No MantineProvider dependency. Dropdown renders via portal to document.body
 * so it's never clipped by ancestor overflow.
 */

import { useState, useRef, useEffect, useLayoutEffect } from 'react';
import { createPortal } from 'react-dom';
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
  const dropRef = useRef<HTMLDivElement>(null);
  const cur = options.find((o) => o.value === value);
  const hasIcons = options.some((o) => o.icon);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current?.contains(e.target as Node)) return;
      if (dropRef.current?.contains(e.target as Node)) return;
      setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  // Position the portal dropdown relative to the button
  const [pos, setPos] = useState<{ top: number; left: number; width: number; flipUp: boolean }>({ top: 0, left: 0, width: 0, flipUp: false });

  useLayoutEffect(() => {
    if (!open || !ref.current) return;
    const rect = ref.current.getBoundingClientRect();
    const dropH = Math.min(options.length * 28 + 8, 176); // ~6 rows cap estimate
    const spaceBelow = window.innerHeight - rect.bottom - 8;
    const flipUp = dropH > spaceBelow && rect.top > spaceBelow;
    setPos({
      top: flipUp ? rect.top : rect.bottom + 4,
      left: rect.left,
      width: rect.width,
      flipUp,
    });
  }, [open, options.length]);

  // Close on scroll of any ancestor
  useEffect(() => {
    if (!open) return;
    const handler = () => {
      if (!ref.current) return;
      const rect = ref.current.getBoundingClientRect();
      const dropH = Math.min(options.length * 28 + 8, 176);
      const spaceBelow = window.innerHeight - rect.bottom - 8;
      const flipUp = dropH > spaceBelow && rect.top > spaceBelow;
      setPos({ top: flipUp ? rect.top : rect.bottom + 4, left: rect.left, width: rect.width, flipUp });
    };
    window.addEventListener('scroll', handler, true);
    return () => window.removeEventListener('scroll', handler, true);
  }, [open, options.length]);

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
      {open && createPortal(
        <div
          ref={dropRef}
          className={styles.drop}
          style={{
            position: 'fixed',
            left: pos.left,
            top: pos.flipUp ? undefined : pos.top,
            bottom: pos.flipUp ? (window.innerHeight - pos.top + 4) : undefined,
            minWidth: Math.max(pos.width, 160),
          }}
        >
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
        </div>,
        document.body,
      )}
    </div>
  );
}
