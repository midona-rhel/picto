import { useEffect, useState } from 'react';
import { IconChevronRight } from '@tabler/icons-react';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { ActionButton } from './ActionButton';
import type { SubscriptionSiteInfo } from '../../../shared/types/subscriptions';
import styles from './NewSubscriptionDialog.module.css';

export interface CreateSubscriptionInput {
  name: string;
  siteId: string;
  queryText: string;
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
  sites,
  onCreate,
  onClose,
}: {
  open: boolean;
  busy: boolean;
  sites: SubscriptionSiteInfo[];
  onCreate: (result: CreateSubscriptionInput) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState('');
  const [siteId, setSiteId] = useState(sites[0]?.id ?? '');
  const [queryText, setQueryText] = useState('');
  const [initialLimit, setInitialLimit] = useState('100');
  const [periodicLimit, setPeriodicLimit] = useState('50');
  const [showAdvanced, setShowAdvanced] = useState(false);

  useEffect(() => {
    if (sites.length > 0 && !sites.some((site) => site.id === siteId)) {
      setSiteId(sites[0].id);
    }
  }, [siteId, sites]);

  const canCreate = name.trim() !== '' && siteId !== '' && queryText.trim() !== '' && !busy;

  const reset = () => {
    setName('');
    setSiteId(sites[0]?.id ?? '');
    setQueryText('');
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
      siteId,
      queryText: queryText.trim(),
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

        <div className={styles.row}>
          <span className={styles.rowLabel}>Source</span>
          <div className={styles.rowControl}>
            <select
              className={styles.textInput}
              value={siteId}
              onChange={(event) => setSiteId(event.target.value)}
              disabled={busy || sites.length === 0}
            >
              {sites.map((site) => <option key={site.id} value={site.id}>{site.name}</option>)}
            </select>
          </div>
        </div>

        <div className={styles.row}>
          <span className={styles.rowLabel}>Query</span>
          <div className={styles.rowControl}>
            <input
              className={styles.textInput}
              value={queryText}
              placeholder={sites.find((site) => site.id === siteId)?.example_query ?? 'artist or tag'}
              onChange={(event) => setQueryText(event.target.value)}
              disabled={busy || sites.length === 0}
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
