import { useRef, useMemo } from 'react';
import {
  IconEdit,
  IconLock,
  IconPlayerPause,
  IconPlayerPlay,
  IconPlayerStop,
  IconPlus,
  IconTrash,
} from '@tabler/icons-react';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import type {
  CredentialDomain,
  CredentialHealth,
  FailedPostGroup,
  SubscriptionInfo,
  SubscriptionProgressEvent,
  SubscriptionSiteInfo,
} from '../../../shared/types/subscriptions';
import {
  formatRelativeTime,
  getQueryAuthState,
  getQueryFailedCount,
  getQueryModeLabel,
  getQueryResumeSummary,
  getSiteLabel,
} from '../subscriptionUtils';
import { ActionButton } from './ActionButton';
import { StatusBadge } from './StatusBadge';
import styles from '../SubscriptionsScreen.module.css';

function summarizeNotes(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  if (!trimmed) return null;
  return trimmed.length > 80 ? `${trimmed.slice(0, 77)}...` : trimmed;
}

export function QueriesTab({
  subscription,
  sites,
  credentials,
  credentialHealth,
  currentProgress,
  failedPosts,
  querySiteId,
  queryDraft,
  queryAddBusy,
  onQuerySiteIdChange,
  onQueryDraftChange,
  onAddQuery,
  onRunQuery,
  onStopQuery,
  onPauseQuery,
  onResetQuery,
  onOpenAuth,
  onDeleteQuery,
  onSaveQueryEdit,
}: {
  subscription: SubscriptionInfo;
  sites: SubscriptionSiteInfo[];
  credentials: CredentialDomain[];
  credentialHealth: CredentialHealth[];
  currentProgress: SubscriptionProgressEvent | null;
  failedPosts: FailedPostGroup[];
  querySiteId: string;
  queryDraft: string;
  queryAddBusy: boolean;
  onQuerySiteIdChange: (value: string) => void;
  onQueryDraftChange: (value: string) => void;
  onAddQuery: () => Promise<void>;
  onRunQuery: (queryId: string) => Promise<void>;
  onStopQuery: (queryId: string) => Promise<void>;
  onPauseQuery: (queryId: string, paused: boolean) => Promise<void>;
  onResetQuery: (queryId: string, label: string) => Promise<void>;
  onOpenAuth: (siteId: string) => void;
  onDeleteQuery: (queryId: string, label: string) => Promise<void>;
  onSaveQueryEdit: (
    queryId: string,
    siteId: string,
    queryText: string,
    displayName: string,
    notes: string,
  ) => Promise<void>;
}) {
  const addInputRef = useRef<HTMLInputElement>(null);
  const activeStandaloneQueryId = currentProgress?.mode === 'query' ? currentProgress.query_id ?? null : null;
  const siteOptions = useMemo(
    () => sites.map((s) => ({ value: s.id, label: getSiteLabel(s.id, sites) })),
    [sites],
  );

  return (
    <div className={styles.section}>
      <div className={styles.queryTable}>
        <div className={styles.queryTableHeader}>
          <span className={styles.qColSite}>Site</span>
          <span className={styles.qColQuery}>Query</span>
          <span className={styles.qColStatus}>State</span>
          <span className={styles.qColNum}>Posts</span>
          <span className={styles.qColNum}>Files</span>
          <span className={styles.qColTime}>Last check</span>
          <span className={styles.qColActions} />
        </div>

        {subscription.queries.map((query) => {
          const label = query.display_name?.trim() || query.query_text;
          const authState = getQueryAuthState({ query, sites, credentials, credentialHealth });
          const failedCount = getQueryFailedCount(query.id, failedPosts);
          const isRunning = activeStandaloneQueryId === query.id;
          const stateTone = isRunning ? 'running'
            : query.paused ? 'paused'
            : authState.blocking || failedCount > 0 || Boolean(query.last_failure_at) ? 'attention'
            : 'idle';
          const stateLabel = isRunning ? (currentProgress?.phase ?? 'running')
            : query.paused ? 'paused'
            : getQueryModeLabel(query);
          const canRun = !activeStandaloneQueryId && !authState.blocking;
          const noteSummary = summarizeNotes(query.notes);

          return (
            <div key={query.id} className={styles.queryCard}>
              {/* Header: site + status */}
              <div className={styles.queryCardHeader}>
                <CmSelect value={query.site_id} options={siteOptions} onChange={(val) => {
                  void onSaveQueryEdit(query.id, val, query.query_text, query.display_name ?? '', query.notes ?? '');
                }} />
                <StatusBadge tone={stateTone} label={stateLabel} />
              </div>

              {/* Query text — editable */}
              <input
                className={styles.queryCardInput}
                defaultValue={query.query_text}
                title={getQueryResumeSummary(query)}
                onBlur={(e) => {
                  const text = e.currentTarget.value.trim();
                  if (text && text !== query.query_text) {
                    void onSaveQueryEdit(query.id, query.site_id, text, query.display_name ?? '', query.notes ?? '');
                  }
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') e.currentTarget.blur();
                  if (e.key === 'Escape') { e.currentTarget.value = query.query_text; e.currentTarget.blur(); }
                }}
              />

              {/* Stats */}
              <div className={styles.queryCardStats}>
                <span>{query.posts_found.toLocaleString()} posts</span>
                <span>{query.files_found.toLocaleString()} files</span>
                {query.last_check_time && <span>checked {formatRelativeTime(query.last_check_time)}</span>}
                {failedCount > 0 && <span>{failedCount} failed</span>}
                {noteSummary && <span>{noteSummary}</span>}
              </div>

              {/* Actions — real buttons */}
              <div className={styles.queryCardActions}>
                <ActionButton compact disabled={isRunning ? false : !canRun} onClick={() => {
                  if (isRunning) void onStopQuery(query.id); else void onRunQuery(query.id);
                }}>
                  {isRunning ? <><IconPlayerStop size={14} /> Stop</> : <><IconPlayerPlay size={14} /> Run</>}
                </ActionButton>
                <ActionButton compact onClick={() => { void onPauseQuery(query.id, !query.paused); }}>
                  <IconPlayerPause size={14} /> {query.paused ? 'Resume' : 'Pause'}
                </ActionButton>
                <ActionButton variant="ghost" compact onClick={() => { void onResetQuery(query.id, label); }}>
                  Reset
                </ActionButton>
                <ActionButton variant="ghost" compact onClick={() => {
                  const nextDisplayName = window.prompt('Query label', query.display_name ?? query.query_text);
                  if (nextDisplayName == null) return;
                  const nextNotes = window.prompt('Query notes', query.notes ?? '');
                  if (nextNotes == null) return;
                  void onSaveQueryEdit(query.id, query.site_id, query.query_text, nextDisplayName, nextNotes);
                }}>
                  <IconEdit size={14} /> Edit
                </ActionButton>
                {authState.blocking && (
                  <ActionButton compact onClick={() => onOpenAuth(query.site_id)}>
                    <IconLock size={14} /> Auth
                  </ActionButton>
                )}
                <ActionButton variant="danger" compact onClick={() => { void onDeleteQuery(query.id, label); }}>
                  <IconTrash size={14} /> Delete
                </ActionButton>
              </div>
            </div>
          );
        })}

        <div className={styles.queryCardAdd}>
          <div className={styles.queryCardHeader}>
            <CmSelect value={querySiteId} options={siteOptions} onChange={onQuerySiteIdChange} />
          </div>
          <input
            ref={addInputRef}
            className={styles.queryCardInput}
            value={queryDraft}
            placeholder={sites.find((s) => s.id === querySiteId)?.example_query ?? 'Enter query...'}
            onChange={(e) => onQueryDraftChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && queryDraft.trim()) void onAddQuery();
            }}
          />
          <div className={styles.queryCardActions}>
            <ActionButton variant="primary" compact disabled={queryAddBusy || !queryDraft.trim()} onClick={() => { void onAddQuery(); }}>
              <IconPlus size={14} /> Add Query
            </ActionButton>
          </div>
        </div>
      </div>
    </div>
  );
}
