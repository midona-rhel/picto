import { useState } from 'react';
import { IconChevronRight } from '@tabler/icons-react';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { ActionButton } from './ActionButton';
import styles from './NewSubscriptionDialog.module.css';

export interface CreateSubscriptionInput {
  name: string;
  initialPostLimit: number;
  periodicPostLimit: number;
}

/**
 * Compact single-form dialog for adding one subscription.
 * Sync limits live under Advanced.
 */
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
  const [initialLimit, setInitialLimit] = useState('100');
  const [periodicLimit, setPeriodicLimit] = useState('50');
  const [showAdvanced, setShowAdvanced] = useState(false);

  const canCreate = name.trim() !== '' && !busy;

  const reset = () => {
    setName('');
    setInitialLimit('100');
    setPeriodicLimit('50');
    setShowAdvanced(false);
  };

  const close = () => {
    reset();
    onClose();
  };

  const submit = () => {
    const parsedInitial = Number.parseInt(initialLimit, 10);
    const parsedPeriodic = Number.parseInt(periodicLimit, 10);
    onCreate({
      name: name.trim(),
      initialPostLimit: Number.isFinite(parsedInitial) && parsedInitial > 0 ? parsedInitial : 100,
      periodicPostLimit: Number.isFinite(parsedPeriodic) && parsedPeriodic > 0 ? parsedPeriodic : 50,
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

        <button
          type="button"
          className={styles.advancedToggle}
          aria-expanded={showAdvanced}
          onClick={() => setShowAdvanced((v) => !v)}
        >
          <IconChevronRight
            size={12}
            className={`${styles.advancedChevron} ${showAdvanced ? styles.advancedChevronOpen : ''}`.trim()}
          />
          Advanced
        </button>

        {showAdvanced && (
          <div className={styles.advancedBody}>
            <div className={styles.row}>
              <span className={styles.rowLabel}>First sync</span>
              <div className={styles.rowControl}>
                <input
                  className={`${styles.textInput} ${styles.numInput}`}
                  value={initialLimit}
                  inputMode="numeric"
                  onChange={(e) => setInitialLimit(e.target.value)}
                />
                <span className={styles.helper}>posts on the first run</span>
              </div>
            </div>
            <div className={styles.row}>
              <span className={styles.rowLabel}>Later checks</span>
              <div className={styles.rowControl}>
                <input
                  className={`${styles.textInput} ${styles.numInput}`}
                  value={periodicLimit}
                  inputMode="numeric"
                  onChange={(e) => setPeriodicLimit(e.target.value)}
                />
                <span className={styles.helper}>new posts per check</span>
              </div>
            </div>
          </div>
        )}
      </div>
    </GlassModal>
  );
}
