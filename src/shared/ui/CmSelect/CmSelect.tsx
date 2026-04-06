import { getZoomFactor, rectToCSS, getViewportCSS } from '../../lib/zoomCompensation';

/**
 * CmSelect — custom dropdown select for use anywhere (portals, scroll containers).
 * No MantineProvider dependency. Dropdown renders via portal to document.body
 * so it's never clipped by ancestor overflow.
 *
 * Button auto-sizes to the widest option label (rendered invisibly).
 * Dropdown always matches the button width.
 * Options include an invisible chevron spacer so text aligns with the button.
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
  /** Fixed width in px. Overrides auto-sizing. */
  width?: number;
}

export function CmSelect({ value, options, onChange, width }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  const dropRef = useRef<HTMLDivElement>(null);
  const cur = options.find((o) => o.value === value);
  const hasIcons = options.some((o) => o.icon);

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

  const [pos, setPos] = useState<{ top: number; left: number; width: number; flipUp: boolean }>({ top: 0, left: 0, width: 0, flipUp: false });

  useLayoutEffect(() => {
    if (!open || !btnRef.current) return;
    const zoom = getZoomFactor();
    const cssRect = rectToCSS(btnRef.current.getBoundingClientRect(), zoom);
    const { height: vh } = getViewportCSS(zoom);
    const dropH = Math.min(options.length * 26 + 8, 164);
    const spaceBelow = vh - cssRect.bottom - 8;
    const flipUp = dropH > spaceBelow && cssRect.top > spaceBelow;
    setPos({ top: flipUp ? cssRect.top : cssRect.bottom + 4, left: cssRect.left, width: cssRect.width, flipUp });
  }, [open, options.length]);

  useEffect(() => {
    if (!open) return;
    const handler = () => {
      if (!btnRef.current) return;
      const zoom = getZoomFactor();
      const cssRect = rectToCSS(btnRef.current.getBoundingClientRect(), zoom);
      const { height: vh } = getViewportCSS(zoom);
      const dropH = Math.min(options.length * 26 + 8, 164);
      const spaceBelow = vh - cssRect.bottom - 8;
      const flipUp = dropH > spaceBelow && cssRect.top > spaceBelow;
      setPos({ top: flipUp ? cssRect.top : cssRect.bottom + 4, left: cssRect.left, width: cssRect.width, flipUp });
    };
    window.addEventListener('scroll', handler, true);
    return () => window.removeEventListener('scroll', handler, true);
  }, [open, options.length]);

  return (
    <div ref={ref} className={styles.root}>
      <button
        ref={btnRef}
        className={styles.btn}
        style={width ? { width } : undefined}
        onClick={(e) => { e.stopPropagation(); setOpen(!open); }}
        type="button"
      >
        {cur?.icon && <span className={styles.btnIcon}>{cur.icon}</span>}
        <span className={styles.btnLabel}>
          {cur?.label ?? value}
          {/* Invisible sizers: force label area to widest option text */}
          {options.map((o) => (
            <span key={o.value} className={styles.sizer} aria-hidden>{o.label}</span>
          ))}
        </span>
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
            bottom: pos.flipUp ? (getViewportCSS().height - pos.top + 4) : undefined,
            width: pos.width,
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
              <span className={styles.optLabel}>{o.label}</span>
            </button>
          ))}
        </div>,
        document.body,
      )}
    </div>
  );
}
