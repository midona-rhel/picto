import { useEffect, useMemo, useState } from 'react';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ActionButton } from './ActionButton';
import { TagAutocompleteInput } from './TagAutocompleteInput';
import type { SubscriptionSiteInfo } from '../../../shared/types/subscriptions';
import styles from '../SubscriptionsScreen.module.css';

export function AddQueryBar({
  sites,
  busy,
  onAdd,
}: {
  sites: SubscriptionSiteInfo[];
  busy: boolean;
  onAdd: (siteId: string, queryText: string) => Promise<void>;
}) {
  const sortedSites = useMemo(
    () => sites
      .filter((site) => site.id !== 'ehentai')
      .sort((left, right) => left.name.localeCompare(right.name, undefined, { sensitivity: 'base' })),
    [sites],
  );
  const [siteId, setSiteId] = useState(sortedSites[0]?.id ?? '');
  const [queryText, setQueryText] = useState('');
  const site = sortedSites.find((entry) => entry.id === siteId) ?? null;

  useEffect(() => {
    setSiteId((current) => (
      sortedSites.some((entry) => entry.id === current)
        ? current
        : (sortedSites[0]?.id ?? '')
    ));
  }, [sortedSites]);

  const submit = async () => {
    const text = queryText.trim();
    if (!siteId || !text || busy) return;
    await onAdd(siteId, text);
    setQueryText('');
  };

  return (
    <div className={styles.addQueryRow}>
      <CmSelect
        value={siteId}
        options={sortedSites.map((entry) => ({ value: entry.id, label: entry.name }))}
        onChange={setSiteId}
        width={140}
        ariaLabel="Source"
      />
      <TagAutocompleteInput
        siteId={siteId}
        value={queryText}
        onChange={setQueryText}
        onSubmit={submit}
        placeholder={site ? `e.g. ${site.example_query}` : 'query'}
      />
      <ActionButton variant="secondary" compact disabled={busy || !queryText.trim()} onClick={submit}>
        Add
      </ActionButton>
    </div>
  );
}
