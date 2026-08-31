import { useEffect, useMemo, useState } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput, GlassTextarea } from '../../shared/ui/GlassInput/GlassInput';
import { organizeIntoGroup } from '../../platform/entityApi';
import type { GroupCandidate } from '../../state/modals';
import type { EntityTarget } from '../../shared/types/canonical';
import styles from './GroupOrganizerModal.module.css';
import { announceUndoableMutation } from '../../runtime/historyRuntime';
import { t } from '../../i18n';

interface GroupOrganizerModalProps {
  open: boolean;
  target: EntityTarget | null;
  coverRootId: number | null;
  groups: GroupCandidate[];
  initialNotes: string;
  maximumNoteBytes: number;
  onClose: () => void;
  onBeforeSubmit?: () => void;
  onComplete?: (groupId: number) => void;
}

export function GroupOrganizerModal({
  open,
  target,
  coverRootId,
  groups,
  initialNotes,
  maximumNoteBytes,
  onClose,
  onBeforeSubmit,
  onComplete,
}: GroupOrganizerModalProps) {
  const [name, setName] = useState('');
  const [winnerId, setWinnerId] = useState<number | null>(null);
  const [notes, setNotes] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choosingWinner = groups.length > 0;

  useEffect(() => {
    if (!open) return;
    setName('');
    setWinnerId(groups[0]?.collection_id ?? null);
    setNotes(initialNotes);
    setSaving(false);
    setError(null);
  }, [groups, initialNotes, open]);

  const notesBytes = useMemo(() => new TextEncoder().encode(notes).length, [notes]);
  const notesTooLong = notesBytes > maximumNoteBytes;

  const canSubmit = Boolean(
    target && coverRootId != null
    && !saving
    && !notesTooLong
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
    onBeforeSubmit?.();
    try {
      const result = await organizeIntoGroup({
        target,
        cover_root_id: coverRootId,
        winning_collection_id: winner?.collection_id ?? null,
        name: choosingWinner ? null : name.trim(),
        notes: notes.trim() || null,
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
      size="md"
      footer={(
        <>
          <button className={modalStyles.btn} onClick={onClose} disabled={saving} type="button">{t("Cancel")}</button>
          <button data-modal-primary="true" className={`${modalStyles.btn} ${modalStyles.btnPrimary}`} onClick={() => { void submit(); }} disabled={!canSubmit} type="button">
            {saving ? t("Saving...") : submitLabel}
          </button>
        </>
      )}
    >
      {choosingWinner ? (
        <div className={styles.choices}>
          <p className={styles.help}>{t("The selected group keeps its name, cover, order, folders, and lifecycle. Every other selected item is appended to it.")}</p>
          {groups.map((group) => (
            <label className={styles.choice} key={group.collection_id}>
              <input
                type="radio"
                name="group-winner"
                checked={winnerId === group.collection_id}
                onChange={() => setWinnerId(group.collection_id)}
              />
              <span className={styles.choiceName}>{group.label || `Group ${group.collection_id}`}</span>
              <span className={styles.choiceCount}>{group.member_count.toLocaleString()} {t("items")}</span>
            </label>
          ))}
        </div>
      ) : (
        <label className={styles.field}>
          <span>{t("Name")}</span>
          <GlassInput
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); void submit(); } }}
            placeholder={t("Group name")}
          />
        </label>
      )}
      <label className={styles.field}>
        <span>{t("Collection notes")}</span>
        <GlassTextarea
          value={notes}
          onChange={(event) => setNotes(event.target.value)}
          placeholder={t("Optional notes for the collection")}
          rows={8}
        />
        <span className={notesTooLong ? styles.counterError : styles.counter}>
          {notesBytes.toLocaleString()} / {maximumNoteBytes.toLocaleString()} {t("bytes")}</span>
        {notesTooLong && (
          <span className={styles.noteError}>{t("Trim the collection notes before continuing.")}</span>
        )}
      </label>
      {error && <p className={styles.error}>{error}</p>}
    </GlassModal>
  );
}
