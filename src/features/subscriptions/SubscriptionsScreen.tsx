import { useCallback, useEffect, useRef } from 'react';
import { useAtom, useAtomValue } from 'jotai';
import { subscriptionsController } from '../../controllers/subscriptionsController';
import { registerSubscriptionsWorkspaceRefresh } from '../../runtime/subscriptionsSettle';
import type { SubscriptionInfo } from '../../shared/types/subscriptions';
import { AccountsModal } from './components/AccountsModal';
import { GroupDetail } from './components/GroupDetail';
import { SidebarRail } from './components/SidebarRail';
import { SubscriptionDetail } from './components/SubscriptionDetail';
import { EmptyState } from './components/EmptyState';
import { NewSubscriptionWizard, type WizardResult } from './wizard/NewSubscriptionWizard';
import {
  EMPTY_SUBSCRIPTION_DETAIL_STATE,
  subscriptionsAccountsModalAtom,
  subscriptionsBusyKeyAtom,
  subscriptionsDetailAtom,
  subscriptionsDetailTabAtom,
  subscriptionsProgressBySubscriptionIdAtom,
  subscriptionsSelectedGroupAtom,
  subscriptionsSelectedProgressAtom,
  subscriptionsSelectedSubscriptionAtom,
  subscriptionsSelectionAtom,
  subscriptionsWizardAtom,
  subscriptionsWorkspaceErrorAtom,
  subscriptionsWorkspaceLoadingAtom,
  subscriptionsWorkspaceSnapshotAtom,
} from '../../state/subscriptionsWorkspace';
import styles from './SubscriptionsScreen.module.css';

const PROGRESS_POLL_MS = 1500;
/** Keep polling briefly after a user-triggered run so startup is caught. */
const POLL_GRACE_MS = 5000;

export function SubscriptionsScreen() {
  const [snapshot, setSnapshot] = useAtom(subscriptionsWorkspaceSnapshotAtom);
  const [loading, setLoading] = useAtom(subscriptionsWorkspaceLoadingAtom);
  const [error, setError] = useAtom(subscriptionsWorkspaceErrorAtom);
  const [selection, setSelection] = useAtom(subscriptionsSelectionAtom);
  const [activeTab, setActiveTab] = useAtom(subscriptionsDetailTabAtom);
  const [detail, setDetail] = useAtom(subscriptionsDetailAtom);
  const [wizard, setWizard] = useAtom(subscriptionsWizardAtom);
  const [accountsModal, setAccountsModal] = useAtom(subscriptionsAccountsModalAtom);
  const [busyKey, setBusyKey] = useAtom(subscriptionsBusyKeyAtom);
  const selectedSubscription = useAtomValue(subscriptionsSelectedSubscriptionAtom);
  const selectedGroup = useAtomValue(subscriptionsSelectedGroupAtom);
  const selectedProgress = useAtomValue(subscriptionsSelectedProgressAtom);
  const progressBySubscriptionId = useAtomValue(subscriptionsProgressBySubscriptionIdAtom);
  const lastRunTriggerRef = useRef(0);

  const refreshWorkspace = useCallback(async () => {
    setError(null);
    try {
      const next = await subscriptionsController.loadWorkspaceSnapshot();
      setSnapshot(next);
      setSelection((current) => {
        if (current?.kind === 'subscription' && next.subscriptions.some((sub) => sub.id === current.id)) {
          return current;
        }
        if (current?.kind === 'group' && next.groups.some((group) => group.id === current.id)) {
          return current;
        }
        const first = next.subscriptions[0];
        return first ? { kind: 'subscription', id: first.id } : null;
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [setError, setLoading, setSelection, setSnapshot]);

  const refreshDetail = useCallback(async (subscription: SubscriptionInfo) => {
    setDetail((current) => ({ ...current, loading: true, error: null, subscriptionId: subscription.id }));
    try {
      const [runs, issues, failedPosts] = await Promise.all([
        subscriptionsController.listRuns(subscription.id),
        subscriptionsController.listIssues(subscription.id),
        subscriptionsController.listFailedPosts(subscription),
      ]);
      setDetail({ loading: false, error: null, subscriptionId: subscription.id, runs, issues, failedPosts });
    } catch (err) {
      setDetail({
        ...EMPTY_SUBSCRIPTION_DETAIL_STATE,
        error: err instanceof Error ? err.message : String(err),
        subscriptionId: subscription.id,
      });
    }
  }, [setDetail]);

  // Initial load + backend-settle refresh
  useEffect(() => {
    void refreshWorkspace();
    return registerSubscriptionsWorkspaceRefresh(() => void refreshWorkspace());
  }, [refreshWorkspace]);

  // Detail follows the selected subscription
  useEffect(() => {
    if (selectedSubscription) void refreshDetail(selectedSubscription);
  }, [selectedSubscription?.id, refreshDetail]); // eslint-disable-line react-hooks/exhaustive-deps

  // Progress polling — only while something runs or shortly after a trigger
  useEffect(() => {
    const anyRunning = (snapshot?.runningSubscriptionIds.length ?? 0) > 0;
    const withinGrace = Date.now() - lastRunTriggerRef.current < POLL_GRACE_MS;
    if (!anyRunning && !withinGrace) return;
    const timer = setInterval(async () => {
      try {
        const runtime = await subscriptionsController.refreshRuntimeState();
        setSnapshot((current) => (current ? { ...current, ...runtime } : current));
      } catch {
        // transient — next tick retries
      }
    }, PROGRESS_POLL_MS);
    return () => clearInterval(timer);
  }, [snapshot?.runningSubscriptionIds.length, setSnapshot]);

  const act = useCallback(async (key: string, action: () => Promise<unknown>, options?: { refresh?: boolean }) => {
    setBusyKey(key);
    setError(null);
    try {
      await action();
      if (options?.refresh !== false) await refreshWorkspace();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusyKey(null);
    }
  }, [refreshWorkspace, setBusyKey, setError]);

  const markRunTriggered = () => {
    lastRunTriggerRef.current = Date.now();
  };

  const createFromWizard = useCallback(async (result: WizardResult) => {
    await act('wizard', async () => {
      let groupId = result.groupId;
      if (result.newGroupName) {
        const group = await subscriptionsController.createGroup(result.newGroupName);
        groupId = Number.parseInt(group.id, 10);
      }
      const subscription = await subscriptionsController.create({
        name: result.name,
        group_id: groupId,
        initial_post_limit: result.initialPostLimit,
        periodic_post_limit: result.periodicPostLimit,
      });
      if (!result.autoCollections) {
        await subscriptionsController.setAutoCollections(subscription.id, false);
      }
      await subscriptionsController.addQuery(subscription.id, result.siteId, result.queryText);
      if (result.runNow) {
        markRunTriggered();
        await subscriptionsController.run(subscription.id);
      }
      setSelection({ kind: 'subscription', id: subscription.id });
      setWizard({ open: false, initialSiteId: null });
    });
  }, [act, setSelection, setWizard]);

  const busy = busyKey != null;
  const detailController = {
    run: (id: string) => {
      markRunTriggered();
      void act(`run:${id}`, () => subscriptionsController.run(id));
    },
    stop: (id: string) => void act(`stop:${id}`, () => subscriptionsController.stop(id)),
    pause: (id: string, paused: boolean) => void act(`pause:${id}`, () => subscriptionsController.pause(id, paused)),
    reset: (id: string) => {
      if (window.confirm('Reset this subscription? Sync progress and download history will be cleared.')) {
        void act(`reset:${id}`, () => subscriptionsController.reset(id));
      }
    },
    delete: (id: string) => {
      if (window.confirm('Delete this subscription? Downloaded files stay in your library.')) {
        void act(`delete:${id}`, () => subscriptionsController.delete(id));
      }
    },
    setAutoCollections: (id: string, on: boolean) =>
      void act(`autocol:${id}`, () => subscriptionsController.setAutoCollections(id, on)),
    setGroup: (id: string, groupId: number | null) =>
      void act(`setgroup:${id}`, () => subscriptionsController.setSubscriptionGroup(id, groupId)),
    runQuery: (subscriptionId: string, queryId: string) => {
      markRunTriggered();
      void act(`runq:${queryId}`, () => subscriptionsController.runQuery(subscriptionId, queryId));
    },
    stopQuery: (subscriptionId: string, queryId: string) =>
      void act(`stopq:${queryId}`, () => subscriptionsController.stopQuery(subscriptionId, queryId)),
    pauseQuery: (queryId: string, paused: boolean) =>
      void act(`pauseq:${queryId}`, () => subscriptionsController.pauseQuery(queryId, paused)),
    deleteQuery: (queryId: string) => {
      if (window.confirm('Delete this query?')) {
        void act(`delq:${queryId}`, () => subscriptionsController.deleteQuery(queryId));
      }
    },
    editQuery: async (queryId: number, siteId: string, queryText: string, displayName: string | null, notes: string | null) => {
      await act(`editq:${queryId}`, () =>
        subscriptionsController.editQuery(queryId, siteId, queryText, displayName, notes));
    },
    addQuery: async (subscriptionId: string, siteId: string, queryText: string) => {
      await act(`addq:${subscriptionId}`, () =>
        subscriptionsController.addQuery(subscriptionId, siteId, queryText));
    },
    openExternalUrl: (url: string) => void subscriptionsController.openExternalUrl(url),
  };

  return (
    <div className={styles.root}>
      <SidebarRail
        groups={snapshot?.groups ?? []}
        subscriptions={snapshot?.subscriptions ?? []}
        listMetrics={snapshot?.listMetrics ?? {}}
        progressBySubscriptionId={progressBySubscriptionId}
        runningSubscriptionIds={snapshot?.runningSubscriptionIds ?? []}
        selection={selection}
        busy={busy}
        onSelect={setSelection}
        onRunGroup={(id) => {
          markRunTriggered();
          void act(`rungroup:${id}`, () => subscriptionsController.runGroup(id));
        }}
        onStopGroup={(id) => void act(`stopgroup:${id}`, () => subscriptionsController.stopGroup(id))}
        onOpenWizard={() => setWizard({ open: true, initialSiteId: null })}
        onOpenAccounts={() => setAccountsModal({ open: true, focusSiteId: null })}
        onCreateGroup={() => {
          const name = window.prompt('Group name');
          if (name?.trim()) {
            void act('creategroup', () => subscriptionsController.createGroup(name.trim()));
          }
        }}
      />

      <main className={styles.detailPane}>
        {error && <div className={styles.errorBanner}>{error}</div>}
        {loading && !snapshot ? (
          <EmptyState title="Loading…" description="Fetching subscriptions." />
        ) : selectedGroup && snapshot ? (
          <GroupDetail
            group={selectedGroup}
            allSubscriptions={snapshot.subscriptions}
            runningSubscriptionIds={snapshot.runningSubscriptionIds}
            busy={busy}
            onRename={(name) => void act(`renamegroup:${selectedGroup.id}`, () => subscriptionsController.renameGroup(selectedGroup.id, name))}
            onSetSchedule={(schedule) => void act(`schedule:${selectedGroup.id}`, () => subscriptionsController.setGroupSchedule(selectedGroup.id, schedule))}
            onRun={() => {
              markRunTriggered();
              void act(`rungroup:${selectedGroup.id}`, () => subscriptionsController.runGroup(selectedGroup.id));
            }}
            onStop={() => void act(`stopgroup:${selectedGroup.id}`, () => subscriptionsController.stopGroup(selectedGroup.id))}
            onDelete={() => {
              if (window.confirm('Delete this group? Its subscriptions are kept (ungrouped).')) {
                void act(`delgroup:${selectedGroup.id}`, async () => {
                  await subscriptionsController.deleteGroup(selectedGroup.id);
                  setSelection(null);
                });
              }
            }}
            onAddSubscription={(subscriptionId) =>
              void act(`groupadd:${subscriptionId}`, () =>
                subscriptionsController.setSubscriptionGroup(subscriptionId, Number.parseInt(selectedGroup.id, 10)))}
            onRemoveSubscription={(subscriptionId) =>
              void act(`groupdel:${subscriptionId}`, () =>
                subscriptionsController.setSubscriptionGroup(subscriptionId, null))}
            onSelectSubscription={(subscriptionId) => setSelection({ kind: 'subscription', id: subscriptionId })}
          />
        ) : selectedSubscription && snapshot ? (
          <SubscriptionDetail
            subscription={selectedSubscription}
            snapshot={snapshot}
            groups={snapshot.groups}
            progress={selectedProgress}
            detail={detail}
            activeTab={activeTab}
            busy={busy}
            controller={{
              ...detailController,
              retryFailedPosts: (posts) => {
                void act('retryposts', async () => {
                  await subscriptionsController.retryFailedPosts(
                    posts
                      .filter((post) => post.queryId != null)
                      .map((post) => ({
                        subscription_id: selectedSubscription.id,
                        query_id: post.queryId as string,
                        site_id: post.siteId,
                        post_id: post.postId,
                      })),
                  );
                  await refreshDetail(selectedSubscription);
                });
              },
            }}
            onTabChange={setActiveTab}
            onOpenAccounts={(siteId) => setAccountsModal({ open: true, focusSiteId: siteId })}
          />
        ) : (
          <EmptyState
            title="Follow artists and tags"
            description="Create a subscription and new posts will land in your library automatically."
          />
        )}
      </main>

      <NewSubscriptionWizard
        open={wizard.open}
        sites={snapshot?.sites ?? []}
        groups={snapshot?.groups ?? []}
        credentialSiteCategories={new Set((snapshot?.credentials ?? []).map((credential) => credential.site_category))}
        initialSiteId={wizard.initialSiteId}
        busy={busy}
        onOpenAccounts={(siteId) => setAccountsModal({ open: true, focusSiteId: siteId })}
        onCreate={(result) => void createFromWizard(result)}
        onClose={() => setWizard({ open: false, initialSiteId: null })}
      />

      <AccountsModal
        open={accountsModal.open}
        focusSiteId={accountsModal.focusSiteId}
        onClose={() => {
          setAccountsModal({ open: false, focusSiteId: null });
          void refreshWorkspace();
        }}
      />
    </div>
  );
}
