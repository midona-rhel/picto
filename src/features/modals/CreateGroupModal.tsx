/**
 * CreateGroupModal — create a new subscription group.
 */

import { useState, useEffect, useCallback } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { CmSelect } from '../../shared/ui/CmSelect/CmSelect';

const SCHEDULE_OPTIONS = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];

export interface CreateGroupModalProps {
  open: boolean;
  onClose: () => void;
  onCreate: (name: string, schedule: string) => void;
}

export function CreateGroupModal({ open, onClose, onCreate }: CreateGroupModalProps) {
  const [name, setName] = useState('');
  const [schedule, setSchedule] = useState('manual');

  useEffect(() => {
    if (open) { setName(''); setSchedule('manual'); }
  }, [open]);

  const handleCreate = useCallback(() => {
    if (!name.trim()) return;
    onCreate(name.trim(), schedule);
  }, [name, schedule, onCreate]);

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

        <div className={modalStyles.field}>
          <label className={modalStyles.fieldLabel}>Schedule</label>
          <CmSelect value={schedule} options={SCHEDULE_OPTIONS} onChange={setSchedule} width={160} />
        </div>

        <p className={modalStyles.helpText}>
          Groups can contain multiple subscriptions that run on the same schedule.
        </p>
      </div>
    </GlassModal>
  );
}
