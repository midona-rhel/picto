import { useState } from 'react';
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
  const [siteId, setSiteId] = useState(sites[0]?.id ?? '');
  const [queryText, setQueryText] = useState('');
  const site = sites.find((entry) => entry.id === siteId) ?? null;

  const submit = async () => {
    const text = queryText.trim();
    if (!siteId || !text || busy) return;
    await onAdd(siteId, text);
    setQueryText('');
  };

  return (
    <div className={styles.queryCardAdd}>
      <CmSelect
        value={siteId}
        options={sites.map((entry) => ({ value: entry.id, label: entry.name }))}
        onChange={setSiteId}
        width={160}
      />
      <TagAutocompleteInput
        siteId={siteId}
        value={queryText}
        onChange={setQueryText}
        onSubmit={submit}
        placeholder={site ? `e.g. ${site.example_query}` : 'query'}
      />
      <ActionButton variant="secondary" disabled={busy || !queryText.trim()} onClick={submit}>
        Add query
      </ActionButton>
    </div>
  );
}
