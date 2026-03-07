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
import { SubscriptionController } from '../../../controllers/subscriptionController';
import { useRuntimeSyncStore } from '../../../state/runtimeSyncStore';
import { listenRuntimeEvent } from '#desktop/api';
import type { FlowInfo, FlowsWorkingProps, SitePluginInfo, SubProgress } from '../types';
import { SCHEDULE_OPTIONS } from '../types';
import {
  canonicalSiteId,
  hasCredentialForSite,
  formatRelativeTime,
  flattenQueries,
  getLastRan,
  getFlowProgress,
  formatSubscriptionFailureMessage,
} from '../lib/flowUtils';
import st from './FlowsWorking.module.css';

export type { FlowExecutionSummary, FlowResultEntry } from '../types';

export function FlowsWorking({
  flowId: _flowId,
  lastResults,
  onLastResultsChange: _onLastResultsChange,
  onOpenCreateModal,
  showHeader = true,
  layoutMode = 'grid',
  headerTitle = 'Subscriptions',
  refreshToken,
}: FlowsWorkingProps) {
  const ensureInitialized = useRuntimeSyncStore((s) => s.ensureInitialized);
  const runningIds = useRuntimeSyncStore((s) => s.runningSubscriptionIds);
  const runningQueryIds = useRuntimeSyncStore((s) => s.runningQueryIds);
  const subscriptionProgressById = useRuntimeSyncStore((s) => s.subscriptionProgressById);
  const runningFlowIds = useRuntimeSyncStore((s) => s.runningFlowIds);
  const flowProgress = useRuntimeSyncStore((s) => s.flowProgressById);
  const lastSubscriptionFinished = useRuntimeSyncStore((s) => s.lastSubscriptionFinished);
  const lastFlowFinished = useRuntimeSyncStore((s) => s.lastFlowFinished);
  const subscriptionEventSeq = useRuntimeSyncStore((s) => s.subscriptionEventSeq);
  const flowEventSeq = useRuntimeSyncStore((s) => s.flowEventSeq);

  const [flows, setFlows] = useState<FlowInfo[]>([]);
  const [sites, setSites] = useState<SitePluginInfo[]>([]);
  const [credentialSites, setCredentialSites] = useState<Set<string>>(new Set());
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const [flowActionMessage, setFlowActionMessage] = useState<Map<string, string>>(new Map());
  const lastSubFinishKeyRef = useRef<string | null>(null);
  const lastFlowFinishKeyRef = useRef<string | null>(null);

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

  const setFlowMessage = useCallback((flowId: string, message: string) => {
    setFlowActionMessage((prev) => {
      const next = new Map(prev);
      next.set(flowId, message);
      return next;
    });
    // Auto-clear stale status text.
    window.setTimeout(() => {
      setFlowActionMessage((prev) => {
        const next = new Map(prev);
        if (next.get(flowId) === message) next.delete(flowId);
        return next;
      });
    }, 6000);
  }, []);

  const loadData = useCallback(async () => {
    try {
      const [flowsData, sitesData, creds] = await Promise.all([
        SubscriptionController.getFlows<FlowInfo>(),
        SubscriptionController.getSiteCatalog(),
        SubscriptionController.listCredentials().catch(() => []),
      ]);
      setFlows(flowsData);
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
    if (!subscriptionEventSeq && !flowEventSeq) return;
    void loadData();
  }, [subscriptionEventSeq, flowEventSeq, loadData]);

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
    if (!lastFlowFinished) return;
    const key = [
      lastFlowFinished.flow_id,
      lastFlowFinished.status,
      lastFlowFinished.error ?? '',
    ].join(':');
    if (lastFlowFinishKeyRef.current === key) return;
    lastFlowFinishKeyRef.current = key;
    if (lastFlowFinished.status === 'failed' && lastFlowFinished.error) {
      notifyError(lastFlowFinished.error, 'Flow Failed');
    } else {
      notifySuccess('Flow completed', 'Flow');
    }
  }, [lastFlowFinished]);

  const handleRenameCommit = useCallback(async (id: string, newName: string) => {
    try {
      await SubscriptionController.renameFlow({ id, name: newName });
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

  const handleDelete = async (flowId: string) => {
    try {
      await SubscriptionController.deleteFlow({ id: flowId });
      await loadData();
    } catch (error) {
      notifyError(`Failed to delete: ${error}`);
    }
  };

  const handleRun = async (flow: FlowInfo) => {
    try {
      const runnableSubscriptions = flow.subscriptions.filter((sub) => {
        if (sub.paused) return false;
        return sub.queries.some((q) => !q.paused);
      });
      if (runnableSubscriptions.length === 0) {
        setFlowMessage(flow.id, 'No active queries to run');
        notifyInfo(
          `"${flow.name}" has no active subscriptions/queries to run.`,
          'Nothing to Run',
        );
        return;
      }

      const missingAuthSites = flow.subscriptions
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
        setFlowMessage(flow.id, `Missing credentials (will likely fail): ${uniqueSites}`);
        notifyInfo(
          `Missing credentials for: ${uniqueSites}. Run will continue; those queries may fail auth.`,
          'Credentials Missing',
        );
      }

      setFlowMessage(flow.id, 'Starting…');
      await SubscriptionController.runFlow({ id: flow.id });
      setFlowMessage(flow.id, 'Run requested');
      notifyInfo(`Started "${flow.name}"`, 'Flow Started');
      await loadData();
    } catch (error) {
      setFlowMessage(flow.id, `Run failed: ${String(error)}`);
      notifyError(`Failed to run: ${error}`);
    }
  };

  const handleStop = async (flow: FlowInfo) => {
    try {
      await SubscriptionController.stopFlow({ id: flow.id });
      notifyInfo(`Stopping "${flow.name}"...`, 'Stopping');
    } catch (error) {
      notifyError(`Failed to stop: ${error}`);
    }
  };

  const handleReset = async (flow: FlowInfo) => {
    try {
      for (const sub of flow.subscriptions) {
        await SubscriptionController.resetSubscription({ id: sub.id });
      }
      notifySuccess(`"${flow.name}" reset. Next run starts fresh.`, 'Reset Complete');
      await loadData();
    } catch (error) {
      notifyError(`Failed to reset: ${error}`);
    }
  };

  const handleScheduleChange = async (flowId: string, schedule: string) => {
    try {
      await SubscriptionController.setFlowSchedule({ id: flowId, schedule });
      setFlows((prev) => prev.map((f) => f.id === flowId ? { ...f, schedule } : f));
    } catch (error) {
      notifyError(`Failed to set schedule: ${error}`);
    }
  };

  const handleDeleteQuery = async (queryId: string) => {
    try {
      await SubscriptionController.deleteSubscriptionQuery({ id: queryId });
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
      await SubscriptionController.runSubscriptionQuery({
        subscriptionId: subId,
        queryId,
      });
      notifyInfo(`Started query "${queryText}"`, 'Query Started');
    } catch (error) {
      notifyError(`Failed to run query: ${error}`);
    }
  };

  const handleAddQuery = async (flowId: string) => {
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

      const flow = flows.find((f) => f.id === flowId);
      const existingSub = flow?.subscriptions.find((s) => (s.site_id ?? s.site_plugin_id) === addSite);
      if (existingSub) {
        await SubscriptionController.addSubscriptionQuery({ subscriptionId: existingSub.id, queryText: addQuery.trim() });
      } else {
        const siteName = sites.find((s) => s.id === addSite)?.name ?? addSite;
        await SubscriptionController.createSubscription({
          name: `${siteName}: ${addQuery.trim()}`,
          siteId: addSite,
          queries: [addQuery.trim()],
          flowId: Number(flowId),
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

      {flows.length === 0 && (
        <EmptyState compact description="No subscriptions yet." />
      )}

      <div className={layoutMode === 'list' ? st.cardList : st.cardGrid}>
        {flows.map((flow) => {
          const isExpanded = expandedIds.has(flow.id);
          const lastRan = getLastRan(flow);
          const lastResult = lastResults[flow.id];
          // PBI-047: Consider flow-level running state too.
          const hasRunningSubscriptions = flow.subscriptions.some((s) => runningIds.has(s.id));
          const isRunning = hasRunningSubscriptions || (runningFlowIds.has(flow.id) && (flowProgress.get(flow.id)?.remaining ?? 1) > 0);
          const queries = flattenQueries(flow, sites, credentialSites);
          const progress = getFlowProgress(flow, progressMap);
          const fp = flowProgress.get(flow.id);
          const actionMessage = flowActionMessage.get(flow.id);

          return (
            <div key={flow.id} className={isRunning ? st.flowCardRunning : st.flowCard}>
              <div className={st.cardTopRow}>
                {renamingId === flow.id ? (
                  <input
                    ref={renameInputRef}
                    className={st.renameInput}
                    value={renameValue}
                    onChange={(e) => setRenameValue(e.target.value)}
                    onBlur={commitRename}
                    onKeyDown={renameKeyHandler}
                  />
                ) : (
                  <span className={st.flowName}>{flow.name}</span>
                )}
                <div className={st.scheduleInline}>
                  <span className={st.scheduleLabel}>Schedule</span>
                  <Select
                    value={flow.schedule}
                    onChange={(value) => { if (value) void handleScheduleChange(flow.id, value); }}
                    data={SCHEDULE_OPTIONS}
                    size="xs"
                    allowDeselect={false}
                    classNames={{ input: st.scheduleInput }}
                  />
                </div>
              </div>

              <div className={st.cardMeta}>
                <span className={st.metaFiles}>{flow.total_files} files</span>
                {!isRunning && <span className={st.metaTime}>Last run: {formatRelativeTime(lastRan)}</span>}
                {lastResult && (
                  <span className={st.metaResult}>
                    Last: {lastResult.added} downloaded · {lastResult.skipped_duplicate} skipped
                  </span>
                )}
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
                  <TextButton compact onClick={() => handleStop(flow)}>
                    <IconPlayerStop size={12} />
                    Stop
                  </TextButton>
                ) : (
                  <TextButton compact onClick={() => handleRun(flow)}>
                    <IconPlayerPlay size={12} />
                    Run
                  </TextButton>
                )}
                <TextButton compact onClick={() => startRename(flow.id, flow.name)}>
                  <IconPencil size={12} />
                  Rename
                </TextButton>
                <TextButton compact onClick={() => handleReset(flow)} disabled={isRunning}>
                  <IconRefresh size={12} />
                  Reset
                </TextButton>
                <TextButton compact danger onClick={() => handleDelete(flow.id)}>
                  <IconTrash size={12} />
                  Delete
                </TextButton>
                <span className={st.actionSpacer} />
                <TextButton compact onClick={() => toggleExpanded(flow.id)}>
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

                  {addingTo === flow.id ? (
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
                          onKeyDown={(e) => { if (e.key === 'Enter') handleAddQuery(flow.id); }}
                          disabled={addLoading}
                          style={{ flex: 2 }}
                        />
                      </div>
                      <div className={st.addQueryActions}>
                        <TextButton compact onClick={() => setAddingTo(null)} disabled={addLoading}>Cancel</TextButton>
                        <TextButton compact onClick={() => handleAddQuery(flow.id)} disabled={!addSite || !addQuery.trim() || addLoading}>Add</TextButton>
                      </div>
                    </div>
                  ) : (
                    <TextButton compact style={{ marginTop: 4 }} onClick={() => { setAddingTo(flow.id); if (sites.length > 0 && !addSite) setAddSite(sites[0].id); setAddQuery(''); }}>
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

