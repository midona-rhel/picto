import { useEffect, useMemo, useState } from 'react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput/GlassInput';
import { organizeIntoCollection } from '../../platform/entityApi';
import type { CollectionCandidate } from '../../state/modals';
import type { ItemTarget } from '../../shared/types/generated/application/ItemTarget';
import styles from './CollectionOrganizerModal.module.css';

interface CollectionOrganizerModalProps {
  open: boolean;
  target: ItemTarget | null;
  collections: CollectionCandidate[];
  onClose: () => void;
  onComplete?: (collectionId: number) => void;
}

export function CollectionOrganizerModal({
  open,
  target,
  collections,
  onClose,
  onComplete,
}: CollectionOrganizerModalProps) {
  const [name, setName] = useState('');
  const [winnerId, setWinnerId] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choosingWinner = collections.length > 0;

  useEffect(() => {
    if (!open) return;
    setName('');
    setWinnerId(collections[0]?.collection_id ?? null);
    setSaving(false);
    setError(null);
  }, [collections, open]);

  const canSubmit = Boolean(
    target
    && !saving
    && (choosingWinner ? winnerId != null : name.trim().length > 0),
  );
  const title = choosingWinner ? 'Choose the Collection to Keep' : 'Create Collection';
  const submitLabel = choosingWinner ? 'Merge Collections' : 'Create Collection';
  const winner = useMemo(
    () => collections.find((collection) => collection.collection_id === winnerId) ?? null,
    [collections, winnerId],
  );

  const submit = async () => {
    if (!target || !canSubmit) return;
    setSaving(true);
    setError(null);
    try {
      const result = await organizeIntoCollection({
        target,
        label: choosingWinner ? null : name.trim(),
        winning_collection_id: winner?.collection_id ?? null,
      });
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
          <button className={`${modalStyles.btn} ${modalStyles.btnPrimary}`} onClick={() => { void submit(); }} disabled={!canSubmit} type="button">
            {saving ? 'Saving...' : submitLabel}
          </button>
        </>
      )}
    >
      {choosingWinner ? (
        <div className={styles.choices}>
          <p className={styles.help}>The selected collection keeps its name, cover, order, folders, and lifecycle. Every other selected item is appended to it.</p>
          {collections.map((collection) => (
            <label className={styles.choice} key={collection.collection_id}>
              <input
                type="radio"
                name="collection-winner"
                checked={winnerId === collection.collection_id}
                onChange={() => setWinnerId(collection.collection_id)}
              />
              <span className={styles.choiceName}>{collection.label || `Collection ${collection.collection_id}`}</span>
              <span className={styles.choiceCount}>{collection.member_count.toLocaleString()} items</span>
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
            placeholder="Collection name"
          />
        </label>
      )}
      {error && <p className={styles.error}>{error}</p>}
    </GlassModal>
  );
}
