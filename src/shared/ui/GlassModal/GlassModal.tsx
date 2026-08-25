/**
 * GlassModal — centered modal with glass surface.
 *
 * Unlike OverlayShell (draggable floating panel), this is a standard
 * centered modal for forms and confirmations.
 */

import { useEffect, useId, useRef, useState, useCallback, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { IconX } from '@tabler/icons-react';
import styles from './GlassModal.module.css';
import btnStyles from '../../styles/actionButton.module.css';
import { useShortcutScope } from '../../hooks/useShortcutScope';

export interface GlassModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  /** Remove default body padding (for edge-to-edge content like trees/lists). */
  flush?: boolean;
  footer?: ReactNode;
  children: ReactNode;
}

const EXIT_MS = 120;

export function GlassModal({ open, onClose, title, size = 'md', flush = false, footer, children }: GlassModalProps) {
  const titleId = useId();
  const [visible, setVisible] = useState(false);
  const [closing, setClosing] = useState(false);
  const backdropPressRef = useRef(false);
  const panelRef = useRef<HTMLDivElement>(null);

  // Sync visibility with open prop
  useEffect(() => {
    if (open) {
      setVisible(true);
      setClosing(false);
    } else if (visible && !closing) {
      // Parent closed us externally (e.g. Cancel button set atom to open:false)
      setVisible(false);
    }
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps -- visible/closing are checked, not deps

  const startClose = useCallback(() => {
    if (closing) return;
    setClosing(true);
    setTimeout(() => { setVisible(false); setClosing(false); onClose(); }, EXIT_MS);
  }, [closing, onClose]);

  useShortcutScope((event) => {
    if (event.key !== 'Escape') return;
    startClose();
    return true;
  }, { enabled: visible, priority: 100, allowInEditable: true });

  if (!visible) return null;

  const sizeClass = size === 'sm'
    ? styles.sm
    : size === 'lg'
      ? styles.lg
      : size === 'xl'
        ? styles.xl
        : styles.md;

  return createPortal(
    <div
      className={`${styles.backdrop} ${closing ? styles.backdropClosing : ''}`}
      onPointerDown={(event) => {
        backdropPressRef.current = event.target === event.currentTarget;
      }}
      onPointerUp={(event) => {
        const closes = backdropPressRef.current && event.target === event.currentTarget;
        backdropPressRef.current = false;
        if (closes) startClose();
      }}
      onPointerCancel={() => { backdropPressRef.current = false; }}
    >
      <div
        ref={panelRef}
        className={`${styles.panel} ${sizeClass} ${closing ? styles.panelClosing : ''}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={(event) => {
          if (event.defaultPrevented) return;
          if (event.key !== 'Enter' || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
          const target = event.target as HTMLElement;
          if (target.closest('button, textarea, [contenteditable="true"], [role="option"], [role="menuitem"]')) return;
          const primary = panelRef.current?.querySelector<HTMLButtonElement>('[data-modal-primary="true"]:not(:disabled)');
          if (!primary) return;
          event.preventDefault();
          primary.click();
        }}
      >
        <div className={styles.header}>
          <span className={styles.title} id={titleId}>{title}</span>
          <button className={styles.closeBtn} onClick={startClose} type="button" title="Close">
            <IconX size={14} />
          </button>
        </div>
        <div className={`${styles.body} ${flush ? styles.bodyFlush : ''}`}>
          {children}
        </div>
        {footer && (
          <div className={styles.footer}>
            {footer}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}

// Re-export style classes for use in modal content (includes shared action buttons)
export const modalStyles = { ...styles, ...btnStyles };
