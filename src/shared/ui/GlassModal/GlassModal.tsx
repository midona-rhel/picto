/**
 * GlassModal — centered modal with glass surface.
 *
 * Unlike OverlayShell (draggable floating panel), this is a standard
 * centered modal for forms and confirmations.
 */

import { useEffect, useState, useCallback, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { IconX } from '@tabler/icons-react';
import styles from './GlassModal.module.css';
import btnStyles from '../../styles/actionButton.module.css';

export interface GlassModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  size?: 'sm' | 'md' | 'lg';
  /** Remove default body padding (for edge-to-edge content like trees/lists). */
  flush?: boolean;
  footer?: ReactNode;
  children: ReactNode;
}

const EXIT_MS = 120;

export function GlassModal({ open, onClose, title, size = 'md', flush = false, footer, children }: GlassModalProps) {
  const [visible, setVisible] = useState(false);
  const [closing, setClosing] = useState(false);

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

  // Escape to close
  useEffect(() => {
    if (!visible) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.stopPropagation(); startClose(); }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [visible, startClose]);

  if (!visible) return null;

  const sizeClass = size === 'sm' ? styles.sm : size === 'lg' ? styles.lg : styles.md;

  return createPortal(
    <div
      className={`${styles.backdrop} ${closing ? styles.backdropClosing : ''}`}
      onClick={startClose}
    >
      <div
        className={`${styles.panel} ${sizeClass} ${closing ? styles.panelClosing : ''}`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className={styles.header}>
          <span className={styles.title}>{title}</span>
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
