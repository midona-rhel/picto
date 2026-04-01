/**
 * GlassModal — centered modal with glass surface.
 *
 * Unlike OverlayShell (draggable floating panel), this is a standard
 * centered modal for forms and confirmations.
 */

import { useEffect, type ReactNode } from 'react';
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

export function GlassModal({ open, onClose, title, size = 'md', flush = false, footer, children }: GlassModalProps) {
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.stopPropagation(); onClose(); }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [open, onClose]);

  if (!open) return null;

  const sizeClass = size === 'sm' ? styles.sm : size === 'lg' ? styles.lg : styles.md;

  return createPortal(
    <div className={styles.backdrop} onClick={onClose}>
      <div
        className={`${styles.panel} ${sizeClass}`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className={styles.header}>
          <span className={styles.title}>{title}</span>
          <button className={styles.closeBtn} onClick={onClose} type="button" title="Close">
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
