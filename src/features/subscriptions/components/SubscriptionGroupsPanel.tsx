import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { useInlineRename } from '../../../shared/hooks/useInlineRename';
import {
  TextInput,
  Select,
  ActionIcon,
  Collapse,
  Tooltip,
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
  IconInfoCircle,
  IconCopy,
  IconCheck,
} from '@tabler/icons-react';
import { useStateChangeStore } from '../../../runtime/stateChanges/stateChangeStore';
import { useSubscriptionProgressStore } from '../../../state-legacy/taskStore';
import { subscriptionsController } from '../../../controllers/subscriptionsController';
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

const SITE_QUERY_HELP: Record<string, { description: string; example: string }> = {
  pixiv:          { description: 'Enter search tags to find artworks.', example: 'landscape' },
  pixivuser:      { description: 'Enter the numeric user ID from the Pixiv profile URL.', example: '12345' },
  gelbooru:       { description: 'Enter space-separated booru tags.', example: 'princess_peach 1girl' },
  danbooru:       { description: 'Enter space-separated booru tags.', example: '1girl solo blue_eyes' },
  rule34:         { description: 'Enter space-separated booru tags.', example: 'solo score:>=50' },
  e621:           { description: 'Enter space-separated booru tags.', example: 'solo canine rating:safe' },
  twitter:        { description: 'Enter the Twitter/X username (without @).', example: 'username' },
  furaffinity:    { description: 'Enter the FurAffinity username. Downloads gallery and scraps.', example: 'username' },
  hentaifoundry:  { description: 'Enter the Hentai Foundry username. Downloads gallery and scraps.', example: 'username' },
  tumblr:         { description: 'Enter the Tumblr blog name (the subdomain).', example: 'blogname' },
  deviantart:     { description: 'Enter the DeviantArt username.', example: 'username' },
  artstation:     { description: 'Enter the ArtStation username.', example: 'username' },
  patreon:        { description: 'Enter the Patreon creator name from their URL.', example: 'creatorname' },
  kemono:         { description: 'Enter the path: service/user/ID. Find the ID from the Kemono page URL.', example: 'patreon/user/12345' },
  coomer:         { description: 'Enter the path: service/user/ID. Find the ID from the Coomer page URL.', example: 'onlyfans/user/12345' },
  fanbox:         { description: 'Enter the Fanbox creator name (their subdomain).', example: 'creatorname' },
  fantia:         { description: 'Enter the Fantia fanclub ID from the URL.', example: '12345' },
  nijie:          { description: 'Enter the numeric user ID from the Nijie profile URL.', example: '12345' },
  instagram:      { description: 'Enter the Instagram username (without @).', example: 'username' },
  sankaku:        { description: 'Enter space-separated booru tags.', example: '1girl' },
  idolcomplex:    { description: 'Enter space-separated booru tags.', example: 'idol' },
  yandere:        { description: 'Enter space-separated booru tags.', example: 'landscape' },
  konachan:       { description: 'Enter space-separated booru tags.', example: 'landscape' },
  safebooru:      { description: 'Enter space-separated booru tags.', example: '1girl smile' },
};

function QueryInfoTooltip({ siteId, sites }: { siteId: string; sites: SitePluginInfo[] }) {
  const si = sites.find((s) => s.id === siteId);
  if (!si) return null;
  const help = SITE_QUERY_HELP[siteId];
  const description = help?.description ?? (si.supports_query ? 'Enter search tags.' : 'Enter a username or ID.');
  const example = help?.example ?? si.example_query;
  return (
    <Tooltip
      label={
        <div style={{ fontSize: 11, lineHeight: 1.4 }}>
          <div>{description}</div>
          <div style={{ marginTop: 4, opacity: 0.7 }}>Example: {example}</div>
          {si.auth_required_for_full_access && (
            <div style={{ marginTop: 6, borderTop: '1px solid rgba(255,255,255,0.15)', paddingTop: 4, opacity: 0.7 }}>
              Credentials recommended for full access.
            </div>
          )}
        </div>
      }
      position="top"
      withArrow
      multiline
      w={240}
    >
      <ActionIcon variant="transparent" size="xs" style={{ flexShrink: 0, color: 'var(--color-text-tertiary)' }}>
        <IconInfoCircle size={14} />
      </ActionIcon>
    </Tooltip>
  );
}

export function SubscriptionGroupsPanel({
  onOpenCreateModal,
  showHeader = true,
  layoutMode = 'grid',
  headerTitle = 'Subscriptions',
  refreshToken,
}: SubscriptionGroupsPanelProps) {
  const ensureInitialized = useStateChangeStore((s) => s.ensureInitialized);
  const subscriptionProgressById = useSubscriptionProgressStore((s) => s.subscriptionProgressById);
  const subscriptionGroupProgress = useSubscriptionProgressStore((s) => s.groupProgressById);
  const lastSubscriptionFinished = useSubscriptionProgressStore((s) => s.lastSubscriptionFinished);
  const lastSubscriptionGroupFinished = useSubscriptionProgressStore((s) => s.lastGroupFinished);
  const subscriptionEventSeq = useSubscriptionProgressStore((s) => s.subscriptionEventSeq);
  const subscriptionGroupEventSeq = useSubscriptionProgressStore((s) => s.groupEventSeq);

  const [subscriptionGroups, setSubscriptionGroups] = useState<SubscriptionGroupInfo[]>([]);
  const [sites, setSites] = useState<SitePluginInfo[]>([]);
  const [credentialSites, setCredentialSites] = useState<Set<string>>(new Set());
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const [subscriptionGroupActionMessage, setSubscriptionGroupActionMessage] = useState<Map<string, string>>(new Map());
  const lastSubFinishKeyRef = useRef<string | null>(null);
  const lastSubscriptionGroupFinishKeyRef = useRef<string | null>(null);

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
    // Auto-clear old status text.
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
        subscriptionsController.listGroups(),
        subscriptionsController.getSites(),
        subscriptionsController.listCredentials().catch(() => []),
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

  const pendingRefreshTargets = useStateChangeStore((s) => s.pendingRefreshTargets);

  useEffect(() => {
    void ensureInitialized();
    loadData();
  }, [ensureInitialized, loadData]);

  useEffect(() => {
    if (!pendingRefreshTargets.has('subscriptions/list')) return;
    void loadData();
    useStateChangeStore.getState().markRefreshTargetHandled('subscriptions/list');
  }, [pendingRefreshTargets, loadData]);

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
      lastSubscriptionGroupFinished.group_id,
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
      await subscriptionsController.renameGroup(id, newName);
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
      await subscriptionsController.deleteGroup(subscriptionGroupId);
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

      const missingAuthSites = runnableSubscriptions
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
      await subscriptionsController.runGroup(subscriptionGroup.id);
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
      await subscriptionsController.stopGroup(subscriptionGroup.id);
      notifyInfo(`Stopping "${subscriptionGroup.name}"...`, 'Stopping');
    } catch (error) {
      notifyError(`Failed to stop: ${error}`);
    }
  };

  const handleReset = async (subscriptionGroup: SubscriptionGroupInfo) => {
    try {
      for (const sub of subscriptionGroup.subscriptions) {
        await subscriptionsController.reset(sub.id);
      }
      notifySuccess(`"${subscriptionGroup.name}" reset. Next run starts fresh.`, 'Reset Complete');
      await loadData();
    } catch (error) {
      notifyError(`Failed to reset: ${error}`);
    }
  };

  const handleScheduleChange = async (subscriptionGroupId: string, schedule: string) => {
    try {
      await subscriptionsController.setGroupSchedule(subscriptionGroupId, schedule);
      setSubscriptionGroups((prev) => prev.map((group) => group.id === subscriptionGroupId ? { ...group, schedule } : group));
    } catch (error) {
      notifyError(`Failed to set schedule: ${error}`);
    }
  };

  const handleDeleteQuery = async (queryId: string) => {
    try {
      await subscriptionsController.deleteQuery(queryId);
      await loadData();
    } catch (error) {
      notifyError(`Failed to delete query: ${error}`);
    }
  };

  const handleSaveEditQuery = async (queryId: string, text?: string) => {
    const value = (text ?? '').trim();
    if (!value) return;
    try {
      await subscriptionsController.editQuery(Number(queryId), value);
      await loadData();
    } catch (error) {
      notifyError(`Failed to update query: ${error}`);
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
      await subscriptionsController.runQuery(subId, queryId);
      notifyInfo(`Started query "${queryText}"`, 'Query Started');
    } catch (error) {
      notifyError(`Failed to run query: ${error}`);
    }
  };

  const handleAddRow = async (subscriptionGroupId: string) => {
    const defaultSite = sites[0]?.id ?? 'pixiv';
    const defaultQuery = 'new_query';
    try {
      const siteName = sites.find((s) => s.id === defaultSite)?.name ?? defaultSite;
      await subscriptionsController.create({
        name: `${siteName}: ${defaultQuery}`,
        site_id: defaultSite,
        queries: [defaultQuery],
        group_id: Number(subscriptionGroupId),
        initial_post_limit: 100,
        periodic_post_limit: 50,
      });
      await loadData();
    } catch (error) {
      notifyError(`Failed to add query: ${error}`);
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
          // Runtime progress keyed by subscription group ID.
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
                <span className={st.metaFiles}>{queries.reduce((s, q) => s + q.filesFound, 0)} files</span>
                <span className={st.metaFiles}>{queries.reduce((s, q) => s + q.postsFound, 0)} posts</span>
                <span className={st.metaFiles}>{queries.length} queries</span>
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
                  {queries.length > 0 && (
                    <div className={st.queryHeaderRow}>
                      <span className={st.queryHeaderCell}>Site</span>
                      <span className={st.queryHeaderCell}>Query</span>
                      <span className={st.queryHeaderCell}>Status</span>
                      <span className={st.queryHeaderCellRight}>Files</span>
                      <span className={st.queryHeaderCellRight}>Posts</span>
                      <span className={st.queryHeaderCellRight}>Cursor</span>
                      <span className={st.queryHeaderCellRight}>Last run</span>
                      <span className={st.queryHeaderCell}></span>
                    </div>
                  )}
                  {queries.map((q) => (
                    <div key={q.queryId} className={st.queryRow}>
                      <div className={st.querySiteCell}>
                        <Select
                          size="xs"
                          variant="unstyled"
                          searchable
                          data={sites.map((si) => ({ value: si.id, label: si.name }))}
                          value={q.sitePluginId}
                          onChange={async (newSiteId) => {
                            if (!newSiteId || newSiteId === q.sitePluginId) return;
                            try {
                              await subscriptionsController.deleteQuery(q.queryId);
                              const sg = subscriptionGroups.find((g) => g.subscriptions.some((s) => s.queries.some((qq) => qq.id === q.queryId)));
                              if (!sg) return;
                              const existingSub = sg.subscriptions.find((s) => (s.site_id ?? s.site_plugin_id) === newSiteId);
                              if (existingSub) {
                                await subscriptionsController.addQuery(existingSub.id, q.queryText);
                              } else {
                                const siteName = sites.find((s) => s.id === newSiteId)?.name ?? newSiteId;
                                await subscriptionsController.create({
                                  name: `${siteName}: ${q.queryText}`,
                                  site_id: newSiteId,
                                  queries: [q.queryText],
                                  group_id: Number(sg.id),
                                  initial_post_limit: 100,
                                  periodic_post_limit: 50,
                                });
                              }
                              await loadData();
                            } catch (error) {
                              notifyError(`Failed to change site: ${error}`);
                            }
                          }}
                          styles={{ input: { fontSize: 11, minHeight: 0, height: 'auto', padding: '0 18px 0 0' } }}
                          comboboxProps={{ width: 'target', offset: 7, styles: { option: { padding: '4px 8px', fontSize: 11, color: 'var(--color-text-primary)' }, dropdown: { padding: 2, marginLeft: -10, width: 'calc(100% + 20px)', backgroundColor: 'var(--color-theme)', backgroundImage: 'linear-gradient(var(--color-white-05), var(--color-white-05))', border: '1px solid var(--color-border-secondary)' } } }}
                          style={{ flex: 1, minWidth: 0 }}
                        />
                        <QueryInfoTooltip siteId={q.sitePluginId} sites={sites} />
                      </div>
                      <TextInput
                        size="xs"
                        variant="unstyled"
                        className={st.queryTextInput}
                        defaultValue={q.queryText}
                        onBlur={(e) => {
                          const text = e.currentTarget.value.trim();
                          if (text && text !== q.queryText) {
                            handleSaveEditQuery(q.queryId, text);
                          }
                        }}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') e.currentTarget.blur();
                          if (e.key === 'Escape') { e.currentTarget.value = q.queryText; e.currentTarget.blur(); }
                        }}
                      />
                      <span className={st.queryStatus}>
                        {q.missingAuth ? (
                          <span className={st.queryAuthWarning}>Missing auth</span>
                        ) : q.completedInitialRun ? (
                          <span className={st.queryStatusDone}><IconCheck size={10} /> Done</span>
                        ) : (
                          <span className={st.queryStatusOk}>Ready</span>
                        )}
                      </span>
                      <span className={st.queryFiles}>{q.filesFound}</span>
                      <span className={st.queryFiles}>{q.postsFound}</span>
                      <span className={st.queryCursor}>{q.resumeCursor ? `@${q.resumeCursor}` : ''}</span>
                      <span className={st.queryTime}>{q.lastCheck ? formatRelativeTime(q.lastCheck) : ''}</span>
                      <div className={st.queryActions}>
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
                        <ActionIcon
                          variant="subtle"
                          color="gray"
                          size="xs"
                          onClick={async () => {
                            try {
                              await subscriptionsController.addQuery(q.backendSubId, q.queryText);
                              await loadData();
                            } catch (error) {
                              notifyError(`Failed to duplicate: ${error}`);
                            }
                          }}
                          title="Duplicate query"
                        >
                          <IconCopy size={12} />
                        </ActionIcon>
                        <ActionIcon variant="subtle" color="gray" size="xs" onClick={() => handleDeleteQuery(q.queryId)}>
                          <IconTrash size={12} />
                        </ActionIcon>
                      </div>
                    </div>
                  ))}

                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginTop: 6 }}>
                    <TextButton compact onClick={() => handleAddRow(subscriptionGroup.id)}>
                      <IconPlus size={12} />
                      Add Query
                    </TextButton>
                    {(() => {
                      const uniqueSubs = new Map<string, boolean>();
                      for (const q of queries) {
                        if (!uniqueSubs.has(q.backendSubId)) {
                          uniqueSubs.set(q.backendSubId, q.autoCollections);
                        }
                      }
                      if (uniqueSubs.size === 0) return null;
                      const [[, firstAutoCollections]] = uniqueSubs;
                      const toggle = async () => {
                        try {
                          for (const subId of uniqueSubs.keys()) {
                            await subscriptionsController.setAutoCollections(subId, !firstAutoCollections);
                          }
                          await loadData();
                        } catch (error) {
                          notifyError(`Failed to update: ${error}`);
                        }
                      };
                      return (
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }} onClick={toggle}>
                          <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)', whiteSpace: 'nowrap' }}>Auto-create collections</span>
                          <label
                            style={{ position: 'relative', display: 'inline-block', width: 32, height: 20, flexShrink: 0, cursor: 'pointer' }}
                            onClick={(e) => { e.stopPropagation(); toggle(); }}
                          >
                            <span style={{
                              position: 'absolute', inset: 0, borderRadius: 10,
                              backgroundColor: firstAutoCollections ? 'var(--color-primary)' : 'rgba(128,128,128,0.25)',
                              boxShadow: 'inset 0 1px 2px rgba(0,0,0,0.1)',
                              transition: 'background-color 0.2s ease',
                            }} />
                            <span style={{
                              position: 'absolute', bottom: 2, left: 2, width: 16, height: 16,
                              borderRadius: '50%', backgroundColor: '#fff',
                              opacity: firstAutoCollections ? 1 : 0.6,
                              boxShadow: '0 1px 3px rgba(0,0,0,0.2)',
                              transform: firstAutoCollections ? 'translateX(12px)' : 'translateX(0)',
                              transition: 'transform 0.2s ease, opacity 0.2s ease',
                            }} />
                          </label>
                        </div>
                      );
                    })()}
                  </div>
                </div>
              </Collapse>
            </div>
          );
        })}
      </div>
    </div>
  );
}
