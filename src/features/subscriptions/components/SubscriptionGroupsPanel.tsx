import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { useInlineRename } from '../../../shared/hooks/useInlineRename';
import {
  TextInput,
  Select,
  ActionIcon,
  Collapse,
} from '@mantine/core';
import { TextButton } from '../../../shared/components/TextButton';
import { EmptyState } from '../../../shared/components/EmptyState';
import { notifySuccess, notifyError, notifyInfo } from '../../../shared/lib/notify';
import {
  IconTrash,
  IconPlus,
  IconPlayerPlay,
  IconPlayerStop,
  IconPencil,
  IconRefresh,
} from '@tabler/icons-react';
import { useRuntimeSyncStore } from '../../../state/runtimeSyncStore';
import { listenRuntimeEvent } from '#desktop/api';
import { subscriptionApi } from '../api';
import type { SubscriptionGroupInfo, SubscriptionGroupsPanelProps, SitePluginInfo, SubProgress } from '../types';
import { SCHEDULE_OPTIONS } from '../types';
import {
  canonicalSiteId,
  hasCredentialForSite,
  formatRelativeTime,
  flattenQueries,
  getLastRan,
  getSubscriptionGroupProgress,
  formatSubscriptionFailureMessage,
} from '../lib/subscriptionGroupUtils';
import st from './SubscriptionGroupsPanel.module.css';

export function SubscriptionGroupsPanel({
  onOpenCreateModal,
  showHeader = true,
  layoutMode = 'grid',
  headerTitle = 'Subscriptions',
  refreshToken,
}: SubscriptionGroupsPanelProps) {
  const ensureInitialized = useRuntimeSyncStore((s) => s.ensureInitialized);
  const subscriptionProgressById = useRuntimeSyncStore((s) => s.subscriptionProgressById);
  const subscriptionGroupProgress = useRuntimeSyncStore((s) => s.flowProgressById);
  const lastSubscriptionFinished = useRuntimeSyncStore((s) => s.lastSubscriptionFinished);
  const lastSubscriptionGroupFinished = useRuntimeSyncStore((s) => s.lastFlowFinished);
  const subscriptionEventSeq = useRuntimeSyncStore((s) => s.subscriptionEventSeq);
  const subscriptionGroupEventSeq = useRuntimeSyncStore((s) => s.flowEventSeq);

  const [subscriptionGroups, setSubscriptionGroups] = useState<SubscriptionGroupInfo[]>([]);
  const [sites, setSites] = useState<SitePluginInfo[]>([]);
  const [credentialSites, setCredentialSites] = useState<Set<string>>(new Set());
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const [subscriptionGroupActionMessage, setSubscriptionGroupActionMessage] = useState<Map<string, string>>(new Map());
  const lastSubFinishKeyRef = useRef<string | null>(null);
  const lastSubscriptionGroupFinishKeyRef = useRef<string | null>(null);

  const [addingTo, setAddingTo] = useState<string | null>(null);
  const [addSite, setAddSite] = useState('');
  const [addQuery, setAddQuery] = useState('');
  const [addLoading, setAddLoading] = useState(false);
  const progressMap = useMemo(() => {
    const next = new Map<string, SubProgress>();
    for (const [subId, progress] of subscriptionProgressById.entries()) {
      next.set(subId, {
        filesDownloaded: progress.files_downloaded,
        filesSkipped: progress.files_skipped,
        pagesFetched: progress.pages_fetched,
        statusText: progress.status_text,
      });
    }
    return next;
  }, [subscriptionProgressById]);
  const runningIds = useMemo(() => {
    const next = new Set<string>();
    for (const [subId, progress] of subscriptionProgressById.entries()) {
      if (progress.status === 'running') next.add(subId);
    }
    return next;
  }, [subscriptionProgressById]);
  const runningQueryIds = useMemo(() => {
    const next = new Set<string>();
    for (const progress of subscriptionProgressById.values()) {
      if (progress.status === 'running' && progress.query_id) next.add(progress.query_id);
    }
    return next;
  }, [subscriptionProgressById]);
  const runningSubscriptionGroupIds = useMemo(() => {
    return new Set(subscriptionGroupProgress.keys());
  }, [subscriptionGroupProgress]);

  const setSubscriptionGroupMessage = useCallback((subscriptionGroupId: string, message: string) => {
    setSubscriptionGroupActionMessage((prev) => {
      const next = new Map(prev);
      next.set(subscriptionGroupId, message);
      return next;
    });
    // Auto-clear stale status text.
    window.setTimeout(() => {
      setSubscriptionGroupActionMessage((prev) => {
        const next = new Map(prev);
        if (next.get(subscriptionGroupId) === message) next.delete(subscriptionGroupId);
        return next;
      });
    }, 6000);
  }, []);

  const loadData = useCallback(async () => {
    try {
      const [subscriptionGroupsData, sitesData, creds] = await Promise.all([
        subscriptionApi.getSubscriptionGroups<SubscriptionGroupInfo>(),
        subscriptionApi.getSiteCatalog(),
        subscriptionApi.listCredentials().catch(() => []),
      ]);
      setSubscriptionGroups(subscriptionGroupsData);
      setSites(sitesData);
      const siteKeys = new Set<string>();
      for (const row of creds) {
        const raw = (row.site_category ?? '').trim().toLowerCase();
        if (!raw) continue;
        siteKeys.add(raw);
        siteKeys.add(canonicalSiteId(raw));
      }
      setCredentialSites(siteKeys);
    } catch (error) {
      console.error('Failed to load subscriptions:', error);
    }
  }, []);

  useEffect(() => {
    void ensureInitialized();
    loadData();
    const unlisten = listenRuntimeEvent('runtime/mutation_committed', (receipt) => {
      if (receipt.facts.domains?.includes('subscriptions')) loadData();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [ensureInitialized, loadData]);

  useEffect(() => {
    if (refreshToken == null) return;
    void loadData();
  }, [refreshToken, loadData]);

  useEffect(() => {
    if (!subscriptionEventSeq && !subscriptionGroupEventSeq) return;
    void loadData();
  }, [subscriptionEventSeq, subscriptionGroupEventSeq, loadData]);

  useEffect(() => {
    if (!lastSubscriptionFinished) return;
    const key = [
      lastSubscriptionFinished.subscription_id,
      lastSubscriptionFinished.query_id ?? '',
      lastSubscriptionFinished.status,
      lastSubscriptionFinished.error ?? '',
      lastSubscriptionFinished.failure_kind ?? '',
      lastSubscriptionFinished.files_downloaded,
      lastSubscriptionFinished.files_skipped,
    ].join(':');
    if (lastSubFinishKeyRef.current === key) return;
    lastSubFinishKeyRef.current = key;
    if (lastSubscriptionFinished.status === 'failed') {
      notifyError(formatSubscriptionFailureMessage(lastSubscriptionFinished), 'Subscription Failed');
    }
  }, [lastSubscriptionFinished]);

  useEffect(() => {
    if (!lastSubscriptionGroupFinished) return;
    const key = [
      lastSubscriptionGroupFinished.flow_id,
      lastSubscriptionGroupFinished.status,
      lastSubscriptionGroupFinished.error ?? '',
    ].join(':');
    if (lastSubscriptionGroupFinishKeyRef.current === key) return;
    lastSubscriptionGroupFinishKeyRef.current = key;
    if (lastSubscriptionGroupFinished.status === 'failed' && lastSubscriptionGroupFinished.error) {
      notifyError(lastSubscriptionGroupFinished.error, 'Subscription Group Failed');
    } else {
      notifySuccess('Subscription group completed', 'Subscription Group');
    }
  }, [lastSubscriptionGroupFinished]);

  const handleRenameCommit = useCallback(async (id: string, newName: string) => {
    try {
      await subscriptionApi.renameSubscriptionGroup({ id, name: newName });
      await loadData();
    } catch (e) { console.error('Rename failed:', e); }
  }, [loadData]);
  const {
    renamingId, renameValue, startRename, setRenameValue,
    commitRename, renameInputRef, renameKeyHandler,
  } = useInlineRename(handleRenameCommit);

  const toggleExpanded = (id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const handleDelete = async (subscriptionGroupId: string) => {
    try {
      await subscriptionApi.deleteSubscriptionGroup({ id: subscriptionGroupId });
      await loadData();
    } catch (error) {
      notifyError(`Failed to delete: ${error}`);
    }
  };

  const handleRun = async (subscriptionGroup: SubscriptionGroupInfo) => {
    try {
      const runnableSubscriptions = subscriptionGroup.subscriptions.filter((sub) => {
        if (sub.paused) return false;
        return sub.queries.some((q) => !q.paused);
      });
      if (runnableSubscriptions.length === 0) {
        setSubscriptionGroupMessage(subscriptionGroup.id, 'No active queries to run');
        notifyInfo(
          `"${subscriptionGroup.name}" has no active subscriptions/queries to run.`,
          'Nothing to Run',
        );
        return;
      }

      const missingAuthSites = subscriptionGroup.subscriptions
        .map((sub) => {
          const siteIdRaw = sub.site_id ?? sub.site_plugin_id ?? '';
          const canonical = canonicalSiteId(siteIdRaw);
          const site = sites.find((s) => canonicalSiteId(s.id) === canonical);
          if (!site?.auth_supported || !site?.auth_required_for_full_access) return null;
          if (hasCredentialForSite(canonical, credentialSites)) return null;
          return site.name || siteIdRaw || canonical;
        })
        .filter((name): name is string => Boolean(name));

      if (missingAuthSites.length > 0) {
        const uniqueSites = Array.from(new Set(missingAuthSites)).join(', ');
        setSubscriptionGroupMessage(subscriptionGroup.id, `Missing credentials (will likely fail): ${uniqueSites}`);
        notifyInfo(
          `Missing credentials for: ${uniqueSites}. Run will continue; those queries may fail auth.`,
          'Credentials Missing',
        );
      }

      setSubscriptionGroupMessage(subscriptionGroup.id, 'Starting…');
      await subscriptionApi.runSubscriptionGroup({ id: subscriptionGroup.id });
      setSubscriptionGroupMessage(subscriptionGroup.id, 'Run requested');
      notifyInfo(`Started "${subscriptionGroup.name}"`, 'Subscription Group Started');
      await loadData();
    } catch (error) {
      setSubscriptionGroupMessage(subscriptionGroup.id, `Run failed: ${String(error)}`);
      notifyError(`Failed to run: ${error}`);
    }
  };

  const handleStop = async (subscriptionGroup: SubscriptionGroupInfo) => {
    try {
      await subscriptionApi.stopSubscriptionGroup({ id: subscriptionGroup.id });
      notifyInfo(`Stopping "${subscriptionGroup.name}"...`, 'Stopping');
    } catch (error) {
      notifyError(`Failed to stop: ${error}`);
    }
  };

  const handleReset = async (subscriptionGroup: SubscriptionGroupInfo) => {
    try {
      for (const sub of subscriptionGroup.subscriptions) {
        await subscriptionApi.resetSubscription({ id: sub.id });
      }
      notifySuccess(`"${subscriptionGroup.name}" reset. Next run starts fresh.`, 'Reset Complete');
      await loadData();
    } catch (error) {
      notifyError(`Failed to reset: ${error}`);
    }
  };

  const handleScheduleChange = async (subscriptionGroupId: string, schedule: string) => {
    try {
      await subscriptionApi.setSubscriptionGroupSchedule({ id: subscriptionGroupId, schedule });
      setSubscriptionGroups((prev) => prev.map((group) => group.id === subscriptionGroupId ? { ...group, schedule } : group));
    } catch (error) {
      notifyError(`Failed to set schedule: ${error}`);
    }
  };

  const handleDeleteQuery = async (queryId: string) => {
    try {
      await subscriptionApi.deleteSubscriptionQuery({ id: queryId });
      await loadData();
    } catch (error) {
      notifyError(`Failed to delete query: ${error}`);
    }
  };

  const handleRunQuery = async (
    subId: string,
    queryId: string,
    queryText: string,
    missingAuth: boolean,
  ) => {
    if (missingAuth) {
      notifyInfo(
        'Missing credentials for this query site. Run will continue and may fail auth.',
        'Credentials Missing',
      );
    }
    try {
      await subscriptionApi.runSubscriptionQuery({
        subscriptionId: subId,
        queryId,
      });
      notifyInfo(`Started query "${queryText}"`, 'Query Started');
    } catch (error) {
      notifyError(`Failed to run query: ${error}`);
    }
  };

  const handleAddQuery = async (subscriptionGroupId: string) => {
    if (!addSite || !addQuery.trim()) return;
    setAddLoading(true);
    try {
      const siteInfo = sites.find((s) => s.id === addSite);
      const needsAuthWarning = Boolean(
        siteInfo?.auth_supported &&
        siteInfo.auth_required_for_full_access &&
        !credentialSites.has(addSite),
      );
      if (needsAuthWarning) {
        notifyInfo(
          `${siteInfo?.name ?? addSite} may return partial/limited data without credentials. You can configure auth in the Subscriptions window.`,
          'Authentication Recommended',
        );
      }

      const subscriptionGroup = subscriptionGroups.find((group) => group.id === subscriptionGroupId);
      const existingSub = subscriptionGroup?.subscriptions.find((s) => (s.site_id ?? s.site_plugin_id) === addSite);
      if (existingSub) {
        await subscriptionApi.addSubscriptionQuery({ subscriptionId: existingSub.id, queryText: addQuery.trim() });
      } else {
        const siteName = sites.find((s) => s.id === addSite)?.name ?? addSite;
        await subscriptionApi.createSubscription({
          name: `${siteName}: ${addQuery.trim()}`,
          siteId: addSite,
          queries: [addQuery.trim()],
          flowId: Number(subscriptionGroupId),
          initialFileLimit: 100,
          periodicFileLimit: 50,
        });
      }
      setAddQuery('');
      setAddingTo(null);
      await loadData();
    } catch (error) {
      notifyError(`Failed to add query: ${error}`);
    } finally {
      setAddLoading(false);
    }
  };

  return (
    <div className={`${st.root} ${!showHeader ? st.rootEmbedded : ''}`.trim()}>
      {showHeader && (
        <div className={st.header}>
          <span className={st.headerTitle}>{headerTitle}</span>
          {onOpenCreateModal && (
            <TextButton compact onClick={onOpenCreateModal}>
              <IconPlus size={12} />
              New
            </TextButton>
          )}
        </div>
      )}

      {subscriptionGroups.length === 0 && (
        <EmptyState compact description="No subscriptions yet." />
      )}

      <div className={layoutMode === 'list' ? st.cardList : st.cardGrid}>
        {subscriptionGroups.map((subscriptionGroup) => {
          const isExpanded = expandedIds.has(subscriptionGroup.id);
          const lastRan = getLastRan(subscriptionGroup);
          // Legacy runtime progress is still keyed as `flow`; map it here until the backend event names are rewritten.
          const hasRunningSubscriptions = subscriptionGroup.subscriptions.some((sub) => runningIds.has(sub.id));
          const isRunning = hasRunningSubscriptions || (runningSubscriptionGroupIds.has(subscriptionGroup.id) && (subscriptionGroupProgress.get(subscriptionGroup.id)?.remaining ?? 1) > 0);
          const queries = flattenQueries(subscriptionGroup, sites, credentialSites);
          const progress = getSubscriptionGroupProgress(subscriptionGroup, progressMap);
          const fp = subscriptionGroupProgress.get(subscriptionGroup.id);
          const actionMessage = subscriptionGroupActionMessage.get(subscriptionGroup.id);

          return (
            <div key={subscriptionGroup.id} className={isRunning ? st.subscriptionGroupCardRunning : st.subscriptionGroupCard}>
              <div className={st.cardTopRow}>
                {renamingId === subscriptionGroup.id ? (
                  <input
                    ref={renameInputRef}
                    className={st.renameInput}
                    value={renameValue}
                    onChange={(e) => setRenameValue(e.target.value)}
                    onBlur={commitRename}
                    onKeyDown={renameKeyHandler}
                  />
                ) : (
                  <span className={st.subscriptionGroupName}>{subscriptionGroup.name}</span>
                )}
                <div className={st.scheduleInline}>
                  <span className={st.scheduleLabel}>Schedule</span>
                  <Select
                    value={subscriptionGroup.schedule}
                    onChange={(value) => { if (value) void handleScheduleChange(subscriptionGroup.id, value); }}
                    data={SCHEDULE_OPTIONS}
                    size="xs"
                    allowDeselect={false}
                    classNames={{ input: st.scheduleInput }}
                  />
                </div>
              </div>

              <div className={st.cardMeta}>
                <span className={st.metaFiles}>{subscriptionGroup.total_files} files</span>
                {!isRunning && <span className={st.metaTime}>Last run: {formatRelativeTime(lastRan)}</span>}
              </div>
              {actionMessage && (
                <div className={st.cardFeedback} aria-live="polite">
                  {actionMessage}
                </div>
              )}

              {isRunning && (
                <div className={st.progressSection}>
                  <div className={st.progressStatus}>
                    {fp
                      ? `${fp.done} / ${fp.total} subscriptions`
                      : progress
                        ? (progress.statusText || `${progress.filesDownloaded} downloaded \u00b7 ${progress.pagesFetched} pages`)
                        : 'Starting...'}
                  </div>
                  {progress && (
                    <div className={st.progressStatusDetail}>
                      {progress.filesDownloaded} downloaded · {progress.filesSkipped} skipped · {progress.pagesFetched} pages
                    </div>
                  )}
                  <div className={st.progressBar}>
                    {fp && fp.total > 0
                      ? <div className={st.progressFill} style={{ width: `${(fp.done / fp.total) * 100}%` }} />
                      : <div className={st.progressIndeterminate} />}
                  </div>
                </div>
              )}

              <div className={st.cardActionsRow}>
                {isRunning ? (
                  <TextButton compact onClick={() => handleStop(subscriptionGroup)}>
                    <IconPlayerStop size={12} />
                    Stop
                  </TextButton>
                ) : (
                  <TextButton compact onClick={() => handleRun(subscriptionGroup)}>
                    <IconPlayerPlay size={12} />
                    Run
                  </TextButton>
                )}
                <TextButton compact onClick={() => startRename(subscriptionGroup.id, subscriptionGroup.name)}>
                  <IconPencil size={12} />
                  Rename
                </TextButton>
                <TextButton compact onClick={() => handleReset(subscriptionGroup)} disabled={isRunning}>
                  <IconRefresh size={12} />
                  Reset
                </TextButton>
                <TextButton compact danger onClick={() => handleDelete(subscriptionGroup.id)}>
                  <IconTrash size={12} />
                  Delete
                </TextButton>
                <span className={st.actionSpacer} />
                <TextButton compact onClick={() => toggleExpanded(subscriptionGroup.id)}>
                  {isExpanded ? 'Hide Queries' : `Queries (${queries.length})`}
                </TextButton>
              </div>

              <Collapse in={isExpanded}>
                <div className={st.expandedBody}>
                  <div className={st.sectionLabel}>Queries ({queries.length})</div>
                  {queries.map((q) => (
                    <div key={q.queryId} className={st.queryRow}>
                      <span className={st.querySite}>{q.siteName}</span>
                      <span className={st.queryText}>{q.queryText}</span>
                      {q.missingAuth && (
                        <span className={st.queryAuthWarning}>Missing auth</span>
                      )}
                      <span className={st.queryFiles}>{q.filesFound}</span>
                      <span className={st.queryTime}>{q.lastCheck ? formatRelativeTime(q.lastCheck) : ''}</span>
                      <ActionIcon
                        variant="subtle"
                        color="gray"
                        size="xs"
                        onClick={() => handleRunQuery(q.backendSubId, q.queryId, q.queryText, q.missingAuth)}
                        disabled={q.paused || runningQueryIds.has(q.queryId)}
                        title={q.paused ? 'Query is paused' : 'Run query'}
                      >
                        <IconPlayerPlay size={12} />
                      </ActionIcon>
                      <ActionIcon variant="subtle" color="gray" size="xs" onClick={() => handleDeleteQuery(q.queryId)}>
                        <IconTrash size={12} />
                      </ActionIcon>
                    </div>
                  ))}

                  {addingTo === subscriptionGroup.id ? (
                    <div>
                      <div className={st.addQueryInputs}>
                        <Select
                          placeholder="Site"
                          size="xs"
                          data={sites.map((si) => ({ value: si.id, label: si.name }))}
                          value={addSite}
                          onChange={(v) => setAddSite(v || '')}
                          disabled={addLoading}
                          style={{ flex: 1 }}
                        />
                        <TextInput
                          placeholder="Query"
                          size="xs"
                          value={addQuery}
                          onChange={(e) => setAddQuery(e.target.value)}
                          onKeyDown={(e) => { if (e.key === 'Enter') handleAddQuery(subscriptionGroup.id); }}
                          disabled={addLoading}
                          style={{ flex: 2 }}
                        />
                      </div>
                      <div className={st.addQueryActions}>
                        <TextButton compact onClick={() => setAddingTo(null)} disabled={addLoading}>Cancel</TextButton>
                        <TextButton compact onClick={() => handleAddQuery(subscriptionGroup.id)} disabled={!addSite || !addQuery.trim() || addLoading}>Add</TextButton>
                      </div>
                    </div>
                  ) : (
                    <TextButton compact style={{ marginTop: 4 }} onClick={() => { setAddingTo(subscriptionGroup.id); if (sites.length > 0 && !addSite) setAddSite(sites[0].id); setAddQuery(''); }}>
                      <IconPlus size={12} />
                      Add Query
                    </TextButton>
                  )}
                </div>
              </Collapse>
            </div>
          );
        })}
      </div>
    </div>
  );
}
