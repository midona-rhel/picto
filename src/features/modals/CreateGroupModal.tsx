/**
 * CreateGroupModal — create a new subscription group.
 */

import { useState, useEffect, useCallback } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';

export interface CreateGroupModalProps {
  open: boolean;
  onClose: () => void;
  onCreate: (name: string) => void;
}

export function CreateGroupModal({ open, onClose, onCreate }: CreateGroupModalProps) {
  const [name, setName] = useState('');

  useEffect(() => {
    if (open) setName('');
  }, [open]);

  const handleCreate = useCallback(() => {
    if (!name.trim()) return;
    onCreate(name.trim());
  }, [name, onCreate]);

  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title="New Subscription Group"
      size="sm"
      footer={
        <>
          <button className={modalStyles.btn} onClick={onClose} type="button">Cancel</button>
          <button
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={handleCreate}
            disabled={!name.trim()}
            type="button"
          >
            Create
          </button>
        </>
      }
    >
      <div className={modalStyles.stack}>
        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Group Name</label>
          <GlassInput
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g., Artists Daily"
            autoFocus
          />
        </div>
        <p className={modalStyles.helpText}>
          Groups organize related subscriptions and provide a manual Run all action.
        </p>
      </div>
    </GlassModal>
  );
}
