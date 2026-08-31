import { useEffect, useId, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useShortcutSuspension } from '../../hooks/useShortcutScope';
import { ProgressBar } from '../ProgressBar';
import styles from './ProgressDialog.module.css';

export interface ProgressDialogProps {
  open: boolean;
  message: string;
  detail?: string | null;
  done?: number;
  total?: number;
  indeterminate?: boolean;
}

export function ProgressDialog({
  open,
  message,
  detail = null,
  done = 0,
  total = 0,
  indeterminate = false,
}: ProgressDialogProps) {
  const labelId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  useShortcutSuspension(open);

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.focus();
    return () => previous?.focus();
  }, [open]);

  if (!open) return null;

  return createPortal(
    <div className={styles.backdrop}>
      <div
        ref={dialogRef}
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelId}
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === 'Tab') event.preventDefault();
        }}
      >
        <div className={styles.content} aria-live="polite">
          <div className={styles.message}>
            <span id={labelId}>{message}</span>
            {detail ? <span className={styles.detail}>{detail}</span> : null}
          </div>
          <div className={styles.progress}>
            <ProgressBar done={done} total={total} indeterminate={indeterminate} height={4} />
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
