import { useEffect, useMemo, useState } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput/GlassInput';
import { organizeIntoGroup } from '../../platform/entityApi';
import type { GroupCandidate } from '../../state/modals';
import type { EntityTarget } from '../../shared/types/canonical';
import styles from './GroupOrganizerModal.module.css';
import { announceUndoableMutation } from '../../runtime/historyRuntime';

interface GroupOrganizerModalProps {
  open: boolean;
  target: EntityTarget | null;
  coverRootId: number | null;
  groups: GroupCandidate[];
  onClose: () => void;
  onComplete?: (groupId: number) => void;
}

export function GroupOrganizerModal({
  open,
  target,
  coverRootId,
  groups,
  onClose,
  onComplete,
}: GroupOrganizerModalProps) {
  const [name, setName] = useState('');
  const [winnerId, setWinnerId] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choosingWinner = groups.length > 0;

  useEffect(() => {
    if (!open) return;
    setName('');
    setWinnerId(groups[0]?.collection_id ?? null);
    setSaving(false);
    setError(null);
  }, [groups, open]);

  const canSubmit = Boolean(
    target && coverRootId != null
    && !saving
    && (choosingWinner ? winnerId != null : name.trim().length > 0),
  );
  const title = choosingWinner ? 'Choose the Group to Keep' : 'Create Group';
  const submitLabel = choosingWinner ? 'Merge Groups' : 'Create Group';
  const winner = useMemo(
    () => groups.find((group) => group.collection_id === winnerId) ?? null,
    [groups, winnerId],
  );

  const submit = async () => {
    if (!target || coverRootId == null || !canSubmit) return;
    setSaving(true);
    setError(null);
    try {
      const result = await organizeIntoGroup({
        target,
        cover_root_id: coverRootId,
        winning_collection_id: winner?.collection_id ?? null,
        name: choosingWinner ? null : name.trim(),
      });
      await announceUndoableMutation('collections.organize');
      onComplete?.(result.collection_id);
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setSaving(false);
    }
  };

  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title={title}
      size="sm"
      footer={(
        <>
          <button className={modalStyles.btn} onClick={onClose} disabled={saving} type="button">Cancel</button>
          <button data-modal-primary="true" className={`${modalStyles.btn} ${modalStyles.btnPrimary}`} onClick={() => { void submit(); }} disabled={!canSubmit} type="button">
            {saving ? 'Saving...' : submitLabel}
          </button>
        </>
      )}
    >
      {choosingWinner ? (
        <div className={styles.choices}>
          <p className={styles.help}>The selected group keeps its name, cover, order, folders, and lifecycle. Every other selected item is appended to it.</p>
          {groups.map((group) => (
            <label className={styles.choice} key={group.collection_id}>
              <input
                type="radio"
                name="group-winner"
                checked={winnerId === group.collection_id}
                onChange={() => setWinnerId(group.collection_id)}
              />
              <span className={styles.choiceName}>{group.label || `Group ${group.collection_id}`}</span>
              <span className={styles.choiceCount}>{group.member_count.toLocaleString()} items</span>
            </label>
          ))}
        </div>
      ) : (
        <label className={styles.field}>
          <span>Name</span>
          <GlassInput
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); void submit(); } }}
            placeholder="Group name"
          />
        </label>
      )}
      {error && <p className={styles.error}>{error}</p>}
    </GlassModal>
  );
}
