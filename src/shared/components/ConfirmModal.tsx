import type { ReactNode } from 'react';
import { Text, Modal } from '@mantine/core';
import { glassModalStyles } from '../styles/glassModal';
import { TextButton } from './TextButton';

interface ConfirmModalProps {
  opened: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  children: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  loading?: boolean;
}

export function ConfirmModal({
  opened, onClose, onConfirm, title, children,
  confirmLabel = 'Confirm', cancelLabel = 'Cancel',
  danger, loading,
}: ConfirmModalProps) {
  return (
    <Modal opened={opened} onClose={onClose} title={title} centered styles={glassModalStyles}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        {typeof children === 'string' ? <Text size="sm">{children}</Text> : children}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 6 }}>
          <TextButton onClick={onClose} disabled={loading}>{cancelLabel}</TextButton>
          <TextButton danger={danger} onClick={onConfirm} disabled={loading}>{confirmLabel}</TextButton>
        </div>
      </div>
    </Modal>
  );
}
