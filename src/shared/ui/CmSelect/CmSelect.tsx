import { getZoomFactor, rectToCSS, getViewportCSS } from '../../lib/zoomCompensation';
import { translateMessage } from '../../../i18n';

/**
 * CmSelect — custom dropdown select for use anywhere (portals, scroll containers).
 * No MantineProvider dependency. Dropdown renders via portal to document.body
 * so it's never clipped by ancestor overflow.
 *
 * Button auto-sizes to the widest option label (rendered invisibly).
 * Dropdown always matches the button width.
 * Options include an invisible chevron spacer so text aligns with the button.
 */

import {
  useState,
  useRef,
  useEffect,
  useId,
  useLayoutEffect,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { IconSelector } from '@tabler/icons-react';
import styles from './CmSelect.module.css';

export interface CmSelectOption {
  value: string;
  label: string;
  icon?: ReactNode;
}

interface Props {
  value: string;
  options: CmSelectOption[];
  onChange: (value: string) => void;
  /** Fixed width in px. Overrides auto-sizing. */
  width?: number;
  ariaLabel?: string;
}

export function CmSelect({ value, options, onChange, width, ariaLabel }: Props) {
  const [open, setOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const ref = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  const dropRef = useRef<HTMLDivElement>(null);
  const typeaheadRef = useRef('');
  const typeaheadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const listboxId = useId();
  const cur = options.find((o) => o.value === value);
  const labelFor = (option: CmSelectOption) => translateMessage(option.label);
  const hasIcons = options.some((o) => o.icon);

  const currentIndex = options.length === 0
    ? -1
    : Math.max(0, options.findIndex((option) => option.value === value));

  const openAt = (index = currentIndex) => {
    if (options.length === 0) return;
    setHighlightedIndex(index);
    setOpen(true);
  };

  const selectHighlighted = () => {
    const option = options[highlightedIndex];
    if (!option) return;
    onChange(option.value);
    setOpen(false);
  };

  const handleKeyDown = (event: ReactKeyboardEvent) => {
    if (options.length === 0) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) {
        openAt(currentIndex);
        return;
      }
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      setHighlightedIndex((index) => Math.max(0, Math.min(options.length - 1, index + delta)));
      return;
    }
    if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      openAt(event.key === 'Home' ? 0 : options.length - 1);
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (open) selectHighlighted();
      else openAt();
      return;
    }
    if (event.key === 'Escape' && open) {
      event.preventDefault();
      setOpen(false);
      return;
    }
    if (event.key === 'Tab') {
      setOpen(false);
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey || event.key.length !== 1 || /\s/.test(event.key)) return;

    event.preventDefault();
    if (typeaheadTimerRef.current) clearTimeout(typeaheadTimerRef.current);
    const nextQuery = `${typeaheadRef.current}${event.key}`.toLocaleLowerCase();
    const match = options.findIndex((option) => labelFor(option).toLocaleLowerCase().startsWith(nextQuery));
    typeaheadRef.current = match >= 0 ? nextQuery : event.key.toLocaleLowerCase();
    const fallbackMatch = match >= 0
      ? match
      : options.findIndex((option) => labelFor(option).toLocaleLowerCase().startsWith(typeaheadRef.current));
    if (fallbackMatch >= 0) openAt(fallbackMatch);
    typeaheadTimerRef.current = setTimeout(() => { typeaheadRef.current = ''; }, 700);
  };

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

  useEffect(() => () => {
    if (typeaheadTimerRef.current) clearTimeout(typeaheadTimerRef.current);
  }, []);

  useEffect(() => {
    if (!open || highlightedIndex < 0) return;
    document.getElementById(`${listboxId}-${highlightedIndex}`)?.scrollIntoView?.({ block: 'nearest' });
  }, [highlightedIndex, listboxId, open]);

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
        onClick={(e) => {
          e.stopPropagation();
          if (open) setOpen(false);
          else openAt();
        }}
        onKeyDown={handleKeyDown}
        type="button"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        {cur?.icon && <span className={styles.btnIcon}>{cur.icon}</span>}
        <span className={styles.btnLabel}>
          {cur ? labelFor(cur) : value}
          {/* Invisible sizers: force label area to widest option text */}
          {options.map((o) => (
            <span key={o.value} className={styles.sizer} aria-hidden>{labelFor(o)}</span>
          ))}
        </span>
        <span className={styles.btnChevron}><IconSelector size={14} /></span>
      </button>
      {open && createPortal(
        <div
          ref={dropRef}
          className={styles.drop}
          role="listbox"
          id={listboxId}
          aria-label={ariaLabel}
          aria-activedescendant={highlightedIndex >= 0 ? `${listboxId}-${highlightedIndex}` : undefined}
          onKeyDown={handleKeyDown}
          style={{
            position: 'fixed',
            left: pos.left,
            top: pos.flipUp ? undefined : pos.top,
            bottom: pos.flipUp ? (getViewportCSS().height - pos.top + 4) : undefined,
            width: pos.width,
          }}
        >
          {options.map((o, index) => (
            <button
              key={o.value}
              id={`${listboxId}-${index}`}
              className={`${styles.opt} ${o.value === value ? styles.optActive : ''} ${index === highlightedIndex ? styles.optHighlighted : ''}`}
              onClick={(e) => { e.stopPropagation(); onChange(o.value); setOpen(false); }}
              onPointerMove={() => setHighlightedIndex(index)}
              type="button"
              role="option"
              aria-selected={o.value === value}
            >
              {hasIcons && <span className={styles.optIcon}>{o.icon ?? null}</span>}
              <span className={styles.optLabel}>{labelFor(o)}</span>
            </button>
          ))}
        </div>,
        document.body,
      )}
    </div>
  );
}
