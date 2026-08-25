import { useEffect, useMemo, useState } from 'react';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ActionButton } from './ActionButton';
import { TagAutocompleteInput } from './TagAutocompleteInput';
import type { SubscriptionQueryInfo, SubscriptionSiteInfo } from '../../../shared/types/subscriptions';
import styles from '../SubscriptionsScreen.module.css';

export function QueryEditModal({
  query,
  sites,
  busy,
  onSave,
  onClose,
}: {
  query: SubscriptionQueryInfo | null;
  sites: SubscriptionSiteInfo[];
  busy: boolean;
  onSave: (input: {
    siteId: string;
    queryText: string;
    displayName: string | null;
    notes: string | null;
  }) => void;
  onClose: () => void;
}) {
  const [siteId, setSiteId] = useState('');
  const [queryText, setQueryText] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [notes, setNotes] = useState('');
  const sortedSites = useMemo(
    () => [...sites].sort((left, right) => left.name.localeCompare(right.name, undefined, { sensitivity: 'base' })),
    [sites],
  );

  useEffect(() => {
    if (!query) return;
    setSiteId(query.site_id);
    setQueryText(query.query_text);
    setDisplayName(query.display_name ?? '');
    setNotes(query.notes ?? '');
  }, [query]);

  const site = sortedSites.find((entry) => entry.id === siteId) ?? null;
  const canSave = siteId !== '' && queryText.trim() !== '' && !busy;
  const queryLabel = site?.supports_query ? 'Search' : 'Account';

  return (
    <GlassModal
      open={query != null}
      onClose={onClose}
      title="Edit query"
      size="md"
      footer={
        <>
          <ActionButton variant="ghost" onClick={onClose}>Cancel</ActionButton>
          <ActionButton
            variant="primary"
            disabled={!canSave}
            onClick={() =>
              onSave({
                siteId,
                queryText: queryText.trim(),
                displayName: displayName.trim() || null,
                notes: notes.trim() || null,
              })
            }
          >
            Save
          </ActionButton>
        </>
      }
    >
      <div className={styles.formField}>
        <span className={styles.label}>Site</span>
        <CmSelect
          value={siteId}
          options={sortedSites.map((entry) => ({ value: entry.id, label: entry.name }))}
          onChange={setSiteId}
          ariaLabel="Source"
        />
      </div>
      <div className={styles.formField}>
        <span className={styles.label}>{queryLabel}</span>
        <TagAutocompleteInput
          siteId={siteId}
          value={queryText}
          onChange={setQueryText}
          placeholder={site?.example_query ?? ''}
        />
      </div>
      <div className={styles.formField}>
        <span className={styles.label}>Display name (optional)</span>
        <GlassInput
          value={displayName}
          placeholder="Shown instead of the query text"
          onChange={(e) => setDisplayName(e.target.value)}
        />
      </div>
      <div className={styles.formField}>
        <span className={styles.label}>Notes (optional)</span>
        <GlassInput value={notes} onChange={(e) => setNotes(e.target.value)} />
      </div>
    </GlassModal>
  );
}
