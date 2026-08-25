/**
 * ConfirmModal — generic confirmation dialog for destructive actions.
 */

import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';

export interface ConfirmModalProps {
  open: boolean;
  onClose: () => void;
  onCancel?: () => void;
  onConfirm: () => void;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  loading?: boolean;
  children?: React.ReactNode;
}

export function ConfirmModal({
  open, onClose, onCancel, onConfirm, title, message,
  confirmLabel = 'Confirm', cancelLabel = 'Cancel',
  danger = false, loading = false, children,
}: ConfirmModalProps) {
  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title={title}
      size="sm"
      footer={
        <>
          <button className={modalStyles.btn} onClick={onCancel ?? onClose} disabled={loading} type="button">
            {cancelLabel}
          </button>
          <button
            className={`${modalStyles.btn} ${danger ? modalStyles.btnDanger : modalStyles.btnPrimary}`}
            onClick={onConfirm}
            disabled={loading}
            type="button"
            data-modal-primary="true"
          >
            {confirmLabel}
          </button>
        </>
      }
    >
      {children ?? (
        <p className={modalStyles.helpText} style={{ fontSize: 13, lineHeight: 1.5 }}>
          {message}
        </p>
      )}
    </GlassModal>
  );
}
