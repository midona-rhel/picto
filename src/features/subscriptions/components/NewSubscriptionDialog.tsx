import { useState } from 'react';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { ActionButton } from './ActionButton';
import styles from './NewSubscriptionDialog.module.css';

export interface CreateSubscriptionInput {
  name: string;
}

/** Compact single-form dialog for adding one subscription. */
export function NewSubscriptionDialog({
  open,
  busy,
  onCreate,
  onClose,
}: {
  open: boolean;
  busy: boolean;
  onCreate: (result: CreateSubscriptionInput) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState('');

  const canCreate = name.trim() !== '' && !busy;

  const reset = () => {
    setName('');
  };

  const close = () => {
    reset();
    onClose();
  };

  const submit = () => {
    onCreate({
      name: name.trim(),
    });
    reset();
  };

  return (
    <GlassModal
      open={open}
      onClose={close}
      title="Add subscription"
      size="sm"
      footer={
        <>
          <ActionButton variant="ghost" onClick={close}>Cancel</ActionButton>
          <ActionButton variant="primary" disabled={!canCreate} onClick={submit}>
            Add
          </ActionButton>
        </>
      }
    >
      <div className={styles.form}>
        <div className={styles.row}>
          <span className={styles.rowLabel}>Name</span>
          <div className={styles.rowControl}>
            <input
              className={styles.textInput}
              value={name}
              placeholder="e.g. Favourite artists"
              autoFocus
              onChange={(e) => setName(e.target.value)}
            />
          </div>
        </div>

      </div>
    </GlassModal>
  );
}
