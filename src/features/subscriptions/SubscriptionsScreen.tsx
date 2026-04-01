import { useEffect, useState } from 'react';
import { useAtom, useAtomValue, useSetAtom } from 'jotai';
import { subscriptionsWorkspaceTabAtom } from '../../state/navigation';
import { AuthWorkspace } from '../auth/AuthWorkspace';
import type { AuthSiteSnapshot } from '../../controllers/authController';
import {
  IconPlayerPause,
  IconPlayerPlay,
  IconPlayerStop,
  IconRotate2,
  IconTrash,
} from '@tabler/icons-react';
import { listen } from '../../platform/ipc';
import { subscriptionsController } from '../../controllers/subscriptionsController';
import type {
  SubscriptionInfo,
} from '../../shared/types/subscriptions';
import { formatDateTime, getSubscriptionSiteSummary } from './subscriptionUtils';
import { ActionButton } from './components/ActionButton';
import { FailedTab } from './components/FailedTab';
import { QueriesTab } from './components/QueriesTab';
import { RunsTab } from './components/RunsTab';
import { SubscriptionsSidebar } from './components/SubscriptionsSidebar';
import { WorkspaceSwitcher } from '../../shared/ui/WorkspaceSwitcher';
import { setSubscriptionsWorkspaceTabAtom } from '../../state/navigation';
import {
  EMPTY_SUBSCRIPTION_CREATE_FORM,
  EMPTY_SUBSCRIPTION_DETAIL_STATE,
  subscriptionsActiveDetailTabAtom,
  subscriptionsBusyKeyAtom,
  subscriptionsCreateBusyAtom,
  subscriptionsCreateFormAtom,
  subscriptionsDetailAtom,
  subscriptionsProgressBySubscriptionIdAtom,
  subscriptionsQueryAddBusyAtom,
  subscriptionsQueryDraftAtom,
  subscriptionsQuerySiteIdAtom,
  subscriptionsSelectedProgressAtom,
  subscriptionsSelectedSubscriptionAtom,
  subscriptionsSelectedSubscriptionIdAtom,
  subscriptionsShowCreateFormAtom,
  subscriptionsWorkspaceErrorAtom,
  subscriptionsWorkspaceLoadingAtom,
  subscriptionsWorkspaceSnapshotAtom,
} from '../../state/subscriptionsWorkspace';
import styles from './SubscriptionsScreen.module.css';

type TabKey = 'queries' | 'failed' | 'runs';

type StateChangedPayload = {
  changes?: {
    domains?: string[];
  };
};

function parseLimit(value: string): number | null {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

export function SubscriptionsScreen() {
  const workspaceTab = useAtomValue(subscriptionsWorkspaceTabAtom);
  const setSubscriptionsWorkspaceTab = useSetAtom(setSubscriptionsWorkspaceTabAtom);
  const [authSites, setAuthSites] = useState<AuthSiteSnapshot[]>([]);
  const [selectedAuthSiteId, setSelectedAuthSiteId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useAtom(subscriptionsWorkspaceSnapshotAtom);
  const [loading, setLoading] = useAtom(subscriptionsWorkspaceLoadingAtom);
  const [error, setError] = useAtom(subscriptionsWorkspaceErrorAtom);
  const [selectedSubscriptionId, setSelectedSubscriptionId] = useAtom(subscriptionsSelectedSubscriptionIdAtom);
  const [activeTab, setActiveTab] = useAtom(subscriptionsActiveDetailTabAtom);
  const [detail, setDetail] = useAtom(subscriptionsDetailAtom);
  const [showCreateForm, setShowCreateForm] = useAtom(subscriptionsShowCreateFormAtom);
  const [createForm, setCreateForm] = useAtom(subscriptionsCreateFormAtom);
  const [createBusy, setCreateBusy] = useAtom(subscriptionsCreateBusyAtom);
  const [querySiteId, setQuerySiteId] = useAtom(subscriptionsQuerySiteIdAtom);
  const [queryDraft, setQueryDraft] = useAtom(subscriptionsQueryDraftAtom);
  const [queryAddBusy, setQueryAddBusy] = useAtom(subscriptionsQueryAddBusyAtom);
  const [busyKey, setBusyKey] = useAtom(subscriptionsBusyKeyAtom);
  const selectedSubscription = useAtomValue(subscriptionsSelectedSubscriptionAtom);
  const progressBySubscriptionId = useAtomValue(subscriptionsProgressBySubscriptionIdAtom);
  const selectedProgress = useAtomValue(subscriptionsSelectedProgressAtom);

  async function refreshWorkspace(options?: { preserveSelection?: boolean }) {
    setError(null);
    if (!snapshot) setLoading(true);
    try {
      const next = await subscriptionsController.loadWorkspaceSnapshot();
      setSnapshot(next);
      setCreateForm((current) => current.name || current.initialPostLimit || current.periodicPostLimit ? current : EMPTY_SUBSCRIPTION_CREATE_FORM);
      setQuerySiteId((current) => current || next.sites[0]?.id || '');
      setSelectedSubscriptionId((current) => {
        if (options?.preserveSelection && current && next.subscriptions.some((subscription) => subscription.id === current)) {
          return current;
        }
        if (current && next.subscriptions.some((subscription) => subscription.id === current)) {
          return current;
        }
        return next.subscriptions[0]?.id ?? null;
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  async function refreshDetail(subscription: SubscriptionInfo) {
    setDetail((current) => ({ ...current, loading: true, error: null, subscriptionId: subscription.id }));
    try {
      const [runs, issues, failedPosts] = await Promise.all([
        subscriptionsController.listRuns(subscription.id),
        subscriptionsController.listIssues(subscription.id),
        subscriptionsController.listFailedPosts(subscription),
      ]);
      setDetail({
        loading: false,
        error: null,
        subscriptionId: subscription.id,
        runs,
        issues,
        failedPosts,
      });
    } catch (err) {
      setDetail({
        loading: false,
        error: err instanceof Error ? err.message : String(err),
        subscriptionId: subscription.id,
        runs: [],
        issues: [],
        failedPosts: [],
      });
    }
  }

  useEffect(() => {
    void refreshWorkspace({ preserveSelection: true });
  }, []);

  useEffect(() => {
    if (!selectedSubscription) {
      setDetail(EMPTY_SUBSCRIPTION_DETAIL_STATE);
      return;
    }
    if (detail.subscriptionId === selectedSubscription.id && !detail.error) return;
    void refreshDetail(selectedSubscription);
  }, [selectedSubscription]);

  useEffect(() => {
    let cancelled = false;
    let previousRunningCount = snapshot?.runningSubscriptionIds.length ?? 0;
    const interval = window.setInterval(() => {
      void subscriptionsController.refreshRuntimeState()
        .then((runtime) => {
          if (cancelled) return;
          setSnapshot((current) => current ? {
            ...current,
            runningSubscriptionIds: runtime.runningSubscriptionIds,
            runningProgress: runtime.runningProgress,
          } : current);
          if (previousRunningCount > 0 && runtime.runningSubscriptionIds.length === 0) {
            void refreshWorkspace({ preserveSelection: true });
          }
          previousRunningCount = runtime.runningSubscriptionIds.length;
        })
        .catch((err) => {
          console.error('Failed to refresh subscription runtime state', err);
        });
    }, 1500);

    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [snapshot?.runningSubscriptionIds.length]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<StateChangedPayload>('runtime/state_changed', (event) => {
      if (cancelled) return;
      const domains = event.payload.changes?.domains ?? [];
      if (domains.includes('subscriptions')) {
        void refreshWorkspace({ preserveSelection: true });
      }
    }).then((dispose) => {
      if (cancelled) {
        dispose();
        return;
      }
      unlisten = dispose;
    }).catch((err) => {
      console.error('Failed to subscribe to subscription state changes', err);
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  async function withBusy(key: string, action: () => Promise<void>) {
    setBusyKey(key);
    try {
      await action();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusyKey(null);
    }
  }

  if (loading) {
    return (
      <div className={styles.detailEmpty}>
        <div className={styles.sectionTitle}>Loading subscriptions…</div>
      </div>
    );
  }

  return (
    <div className={styles.root}>
      <SubscriptionsSidebar
        snapshot={snapshot}
        error={error}
        selectedSubscriptionId={selectedSubscriptionId}
        progressBySubscriptionId={progressBySubscriptionId}
        showCreateForm={showCreateForm}
        createForm={createForm}
        createBusy={createBusy}
        onToggleCreateForm={() => setShowCreateForm((current) => !current)}
        onSelectSubscription={setSelectedSubscriptionId}
        onCreateFormChange={(patch) => setCreateForm((current) => ({ ...current, ...patch }))}
        onCreate={async () => {
          setCreateBusy(true);
          setError(null);
          try {
            const created = await subscriptionsController.create({
              name: createForm.name.trim(),
              initial_post_limit: parseLimit(createForm.initialPostLimit),
              periodic_post_limit: parseLimit(createForm.periodicPostLimit),
            });
            await refreshWorkspace({ preserveSelection: true });
            setSelectedSubscriptionId(created.id);
            setShowCreateForm(false);
            setCreateForm(EMPTY_SUBSCRIPTION_CREATE_FORM);
          } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
          } finally {
            setCreateBusy(false);
          }
        }}
        onCancelCreate={() => {
          setCreateForm(EMPTY_SUBSCRIPTION_CREATE_FORM);
          setShowCreateForm(false);
        }}
        authSites={authSites}
        selectedAuthSiteId={selectedAuthSiteId}
        onSelectAuthSite={setSelectedAuthSiteId}
      />

      {workspaceTab === 'auth' ? (
        <AuthWorkspace
          hideSidebar
          onSitesLoaded={setAuthSites}
          externalSelectedSiteId={selectedAuthSiteId}
          onSelectSite={setSelectedAuthSiteId}
        />
      ) : (
      <main className={styles.content}>
        {!selectedSubscription ? (
            <div className={styles.detailEmpty}>
              <div className={styles.sectionTitle}>Select a subscription</div>
            <div className={styles.muted}>The detail pane shows queries, failed posts, and runs for the selected source.</div>
          </div>
        ) : (
          <>
            <section className={styles.hero}>
              <div className={styles.heroTop}>
                <div className={styles.titleWrap}>
                  <div className={styles.heroTitle}>{selectedSubscription.name}</div>
                  <div className={styles.subtitle}>
                    {getSubscriptionSiteSummary(selectedSubscription.queries, snapshot?.sites ?? [])} · created {formatDateTime(selectedSubscription.created_at)}
                  </div>
                </div>
                <div className={styles.heroActions}>
                  <ActionButton compact disabled={busyKey !== null} onClick={() => {
                    const next = window.prompt('Rename subscription', selectedSubscription.name);
                    if (!next || next.trim() === selectedSubscription.name) return;
                    void withBusy('rename-subscription', async () => {
                      await subscriptionsController.rename(selectedSubscription.id, next.trim());
                      await refreshWorkspace({ preserveSelection: true });
                    });
                  }}>
                    Rename
                  </ActionButton>
                  <ActionButton compact disabled={busyKey !== null || Boolean(selectedProgress)} onClick={() => {
                    void withBusy('run-subscription', async () => {
                      await subscriptionsController.run(selectedSubscription.id);
                      await refreshWorkspace({ preserveSelection: true });
                    });
                  }}>
                    <IconPlayerPlay size={14} />
                    Run
                  </ActionButton>
                  <ActionButton compact disabled={busyKey !== null || !selectedProgress} onClick={() => {
                    void withBusy('stop-subscription', async () => {
                      await subscriptionsController.stop(selectedSubscription.id);
                      await refreshWorkspace({ preserveSelection: true });
                    });
                  }}>
                    <IconPlayerStop size={14} />
                    Stop
                  </ActionButton>
                  <ActionButton compact disabled={busyKey !== null} onClick={() => {
                    void withBusy('pause-subscription', async () => {
                      await subscriptionsController.pause(selectedSubscription.id, !selectedSubscription.paused);
                      await refreshWorkspace({ preserveSelection: true });
                    });
                  }}>
                    <IconPlayerPause size={14} />
                    {selectedSubscription.paused ? 'Resume' : 'Pause'}
                  </ActionButton>
                  <ActionButton variant="ghost" compact disabled={busyKey !== null} onClick={() => {
                    if (!window.confirm(`Reset ${selectedSubscription.name}? This clears subscription progress and archive state.`)) return;
                    void withBusy('reset-subscription', async () => {
                      await subscriptionsController.reset(selectedSubscription.id);
                      await refreshWorkspace({ preserveSelection: true });
                      await refreshDetail(selectedSubscription);
                    });
                  }}>
                    <IconRotate2 size={14} />
                    Reset
                  </ActionButton>
                  <ActionButton variant="danger" compact disabled={busyKey !== null} onClick={() => {
                    if (!window.confirm(`Delete ${selectedSubscription.name}?`)) return;
                    void withBusy('delete-subscription', async () => {
                      await subscriptionsController.delete(selectedSubscription.id);
                      await refreshWorkspace({ preserveSelection: false });
                    });
                  }}>
                    <IconTrash size={14} />
                    Delete
                  </ActionButton>
                </div>
              </div>

              <div className={styles.summaryGrid}>
                <div className={styles.summaryCard}>
                  <div className={styles.summaryLabel}>Status</div>
                  <div className={styles.summaryValue}>{selectedProgress ? 'Running' : selectedSubscription.paused ? 'Paused' : 'Ready'}</div>
                  <div className={styles.muted}>{selectedProgress?.status_text ?? 'Not currently running'}</div>
                </div>
                <div className={styles.summaryCard}>
                  <div className={styles.summaryLabel}>Files</div>
                  <div className={styles.summaryValue}>{selectedSubscription.total_files.toLocaleString()}</div>
                  <div className={styles.muted}>{selectedSubscription.queries.length} queries configured</div>
                </div>
                <div className={styles.summaryCard}>
                  <div className={styles.summaryLabel}>Limits</div>
                  <div className={styles.summaryValue}>{selectedSubscription.initial_post_limit}/{selectedSubscription.periodic_post_limit}</div>
                  <div className={styles.muted}>Initial / periodic post window</div>
                </div>
                <div className={styles.summaryCard}>
                  <div className={styles.summaryLabel}>Collections</div>
                  <div className={styles.summaryValue}>{selectedSubscription.auto_collections ? 'Auto' : 'Manual'}</div>
                  <label className={styles.checkboxRow}>
                    <input
                      type="checkbox"
                      checked={selectedSubscription.auto_collections}
                      onChange={(event) => {
                        void withBusy('auto-collections', async () => {
                          await subscriptionsController.setAutoCollections(selectedSubscription.id, event.target.checked);
                          await refreshWorkspace({ preserveSelection: true });
                        });
                      }}
                    />
                    Auto-create collections
                  </label>
                </div>
              </div>

              <div className={styles.tabList}>
                <WorkspaceSwitcher
                  value={activeTab}
                  onChange={setActiveTab}
                  options={[
                    { value: 'queries' as TabKey, label: 'Queries' },
                    { value: 'failed' as TabKey, label: 'Failed' },
                    { value: 'runs' as TabKey, label: 'Runs' },
                  ]}
                />
              </div>
            </section>

            <section className={styles.tabPanel}>
              {detail.error && <div className={styles.errorBanner}>{detail.error}</div>}

              {activeTab === 'queries' && (
                <QueriesTab
                  subscription={selectedSubscription}
                  sites={snapshot?.sites ?? []}
                  credentials={snapshot?.credentials ?? []}
                  credentialHealth={snapshot?.credentialHealth ?? []}
                  currentProgress={selectedProgress}
                  failedPosts={detail.failedPosts}
                  querySiteId={querySiteId}
                  queryDraft={queryDraft}
                  queryAddBusy={queryAddBusy}
                  onQuerySiteIdChange={setQuerySiteId}
                  onQueryDraftChange={setQueryDraft}
                  onAddQuery={async () => {
                    setQueryAddBusy(true);
                    try {
                      await subscriptionsController.addQuery(selectedSubscription.id, querySiteId, queryDraft.trim(), null);
                      setQueryDraft('');
                      await refreshWorkspace({ preserveSelection: true });
                    } catch (err) {
                      setError(err instanceof Error ? err.message : String(err));
                    } finally {
                      setQueryAddBusy(false);
                    }
                  }}
                  onRunQuery={async (queryId) => {
                    await subscriptionsController.runQuery(selectedSubscription.id, queryId);
                    await refreshWorkspace({ preserveSelection: true });
                  }}
                  onStopQuery={async (queryId) => {
                    await subscriptionsController.stopQuery(selectedSubscription.id, queryId);
                    await refreshWorkspace({ preserveSelection: true });
                  }}
                  onPauseQuery={async (queryId, paused) => {
                    await subscriptionsController.pauseQuery(queryId, paused);
                    await refreshWorkspace({ preserveSelection: true });
                  }}
                  onResetQuery={async (queryId, label) => {
                    if (!window.confirm(`Reset query "${label}"? This clears its progress, retry state, and gallery-dl archive memory.`)) return;
                    await subscriptionsController.resetQuery(queryId);
                    await refreshWorkspace({ preserveSelection: true });
                    await refreshDetail(selectedSubscription);
                  }}
                  onOpenAuth={(siteId) => {
                    setSubscriptionsWorkspaceTab('auth');
                    window.dispatchEvent(new CustomEvent('picto:open-auth-site', { detail: { siteId } }));
                  }}
                  onDeleteQuery={async (queryId, label) => {
                    if (!window.confirm(`Delete query "${label}"?`)) return;
                    await subscriptionsController.deleteQuery(queryId);
                    await refreshWorkspace({ preserveSelection: true });
                  }}
                  onSaveQueryEdit={async (queryId, siteId, queryText, displayName, notes) => {
                    await subscriptionsController.editQuery(Number(queryId), siteId, queryText, displayName || null, notes || null);
                    await refreshWorkspace({ preserveSelection: true });
                  }}
                />
              )}

              {activeTab === 'failed' && (
                <FailedTab
                  failedPosts={detail.failedPosts}
                  loading={detail.loading && detail.subscriptionId === selectedSubscription.id}
                  onOpenExternal={(url) => {
                    void subscriptionsController.openExternalUrl(url);
                  }}
                  onRetryPost={async (failedPost) => {
                    await subscriptionsController.retryFailedPost({
                      subscription_id: selectedSubscription.id,
                      query_id: failedPost.queryId ?? '',
                      site_id: failedPost.siteId,
                      post_id: failedPost.postId,
                    });
                    await refreshWorkspace({ preserveSelection: true });
                    await refreshDetail(selectedSubscription);
                  }}
                />
              )}

              {activeTab === 'runs' && (
                <RunsTab runs={detail.runs} issues={detail.issues} />
              )}
            </section>
          </>
        )}
      </main>
      )}
    </div>
  );
}
