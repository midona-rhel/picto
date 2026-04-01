/**
 * ConfirmModal — generic confirmation dialog for destructive actions.
 */

import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';

export interface ConfirmModalProps {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  loading?: boolean;
}

export function ConfirmModal({
  open, onClose, onConfirm, title, message,
  confirmLabel = 'Confirm', cancelLabel = 'Cancel',
  danger = false, loading = false,
}: ConfirmModalProps) {
  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title={title}
      size="sm"
      footer={
        <>
          <button className={modalStyles.btn} onClick={onClose} disabled={loading} type="button">
            {cancelLabel}
          </button>
          <button
            className={`${modalStyles.btn} ${danger ? modalStyles.btnDanger : modalStyles.btnPrimary}`}
            onClick={onConfirm}
            disabled={loading}
            type="button"
          >
            {confirmLabel}
          </button>
        </>
      }
    >
      <p style={{ margin: 0, color: 'var(--color-text-secondary)', fontSize: 13, lineHeight: 1.5 }}>
        {message}
      </p>
    </GlassModal>
  );
}
