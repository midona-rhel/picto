import { useCallback, useEffect, useState } from 'react';
import { pushSubscriptionsHistory } from '../../state/navigationHistory';
import { useAtom, useAtomValue, useSetAtom } from 'jotai';
import { confirmModalAtom } from '../../state/modals';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { subscriptionsController } from '../../controllers/subscriptionsController';
import {
  buildMultiCardMenu,
  buildSubscriptionMenu,
} from './subscriptionsContextMenu';
import { RenameDialog, type RenameTarget } from './components/RenameDialog';
import {
  markSubscriptionRunTriggered,
  refreshSubscriptionsWorkspace,
  startSubscriptionsSettle,
} from '../../runtime/subscriptionsSettle';
import type { SubscriptionInfo } from '../../shared/types/subscriptions';
import {
  getCredentialOwnerSiteId,
  groupFailedPostAttempts,
} from '../../shared/lib/subscriptionHelpers';
import { AccountsModal } from './components/AccountsModal';
import { SubscriptionsGrid } from './components/SubscriptionsGrid';
import { SubscriptionDetail } from './components/SubscriptionDetail';
import { EmptyState } from './components/EmptyState';
import { NewSubscriptionDialog, type CreateSubscriptionInput } from './components/NewSubscriptionDialog';
import {
  EMPTY_SUBSCRIPTION_DETAIL_STATE,
  subscriptionsAccountsModalAtom,
  subscriptionsBusyKeyAtom,
  subscriptionsCoversAtom,
  subscriptionsDetailAtom,
  subscriptionsDetailTabAtom,
  subscriptionsProgressBySubscriptionIdAtom,
  subscriptionsSelectedProgressAtom,
  subscriptionsSelectedSubscriptionAtom,
  subscriptionsSelectionAtom,
  subscriptionsWizardAtom,
  subscriptionsWorkspaceErrorAtom,
  subscriptionsWorkspaceLoadingAtom,
  subscriptionsWorkspaceSnapshotAtom,
} from '../../state/subscriptionsWorkspace';
import styles from './SubscriptionsScreen.module.css';

export function SubscriptionsScreen() {
  const snapshot = useAtomValue(subscriptionsWorkspaceSnapshotAtom);
  const loading = useAtomValue(subscriptionsWorkspaceLoadingAtom);
  const [error, setError] = useAtom(subscriptionsWorkspaceErrorAtom);
  const [selection, setSelection] = useAtom(subscriptionsSelectionAtom);
  const [activeTab, setActiveTab] = useAtom(subscriptionsDetailTabAtom);
  const [detail, setDetail] = useAtom(subscriptionsDetailAtom);
  const [wizard, setWizard] = useAtom(subscriptionsWizardAtom);
  const [accountsModal, setAccountsModal] = useAtom(subscriptionsAccountsModalAtom);
  const [busyKey, setBusyKey] = useAtom(subscriptionsBusyKeyAtom);
  const selectedSubscription = useAtomValue(subscriptionsSelectedSubscriptionAtom);
  const selectedProgress = useAtomValue(subscriptionsSelectedProgressAtom);
  const progressBySubscriptionId = useAtomValue(subscriptionsProgressBySubscriptionIdAtom);
  const covers = useAtomValue(subscriptionsCoversAtom);
  const contextMenu = useContextMenu();
  const setConfirmModal = useSetAtom(confirmModalAtom);
  const [renameTarget, setRenameTarget] = useState<RenameTarget | null>(null);

  const confirm = useCallback(
    (opts: { title: string; message: string; confirmLabel?: string; danger?: boolean }, action: () => void) => {
      setConfirmModal({ ...opts, open: true, onConfirm: action });
    },
    [setConfirmModal],
  );

  // Errors persist until dismissed — blocked runs carry instructions the user
  // needs time to read; workspace reloads clear them.

  const refreshDetail = useCallback(async (subscription: SubscriptionInfo) => {
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
        issues: issues.items,
        failedPosts: failedPosts.failedPosts,
        attempts: failedPosts.attempts,
        issueNextCursor: issues.next_cursor,
        failedPostNextCursor: failedPosts.nextCursor,
        issueTotalCount: issues.total_count,
        failedPostTotalCount: failedPosts.totalCount,
        retryablePostCount: failedPosts.retryableCount,
      });
    } catch (err) {
      setDetail({
        ...EMPTY_SUBSCRIPTION_DETAIL_STATE,
        error: err instanceof Error ? err.message : String(err),
        subscriptionId: subscription.id,
      });
    }
  }, [setDetail]);

  // This screen is shared by the main and standalone subscription windows.
  useEffect(() => {
    const stopSettle = startSubscriptionsSettle();
    void refreshSubscriptionsWorkspace();
    return stopSettle;
  }, []);

  // Detail follows the selected subscription
  useEffect(() => {
    if (selectedSubscription) void refreshDetail(selectedSubscription);
  }, [selectedSubscription, refreshDetail]);

  // A menu belongs to the surface it was opened on — close it on navigation.
  useEffect(() => {
    contextMenu.close();
  }, [selection?.kind, selection?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const act = useCallback(async (key: string, action: () => Promise<unknown>, options?: { refresh?: boolean }) => {
    setBusyKey(key);
    setError(null);
    try {
      await action();
      if (options?.refresh !== false) await refreshSubscriptionsWorkspace();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusyKey(null);
    }
  }, [setBusyKey, setError]);

  const loadMoreHealth = useCallback(async () => {
    if (!selectedSubscription || busyKey) return;
    const issueCursor = detail.issueNextCursor;
    const attemptCursor = detail.failedPostNextCursor;
    if (issueCursor == null && attemptCursor == null) return;
    await act('health:more', async () => {
      const [issues, failed] = await Promise.all([
        issueCursor == null
          ? Promise.resolve(null)
          : subscriptionsController.listIssues(selectedSubscription.id, issueCursor),
        attemptCursor == null
          ? Promise.resolve(null)
          : subscriptionsController.listFailedPosts(selectedSubscription, attemptCursor),
      ]);
      setDetail((current) => {
        const attempts = failed ? [...current.attempts, ...failed.attempts] : current.attempts;
        return {
          ...current,
          issues: issues ? [...current.issues, ...issues.items] : current.issues,
          attempts,
          failedPosts: groupFailedPostAttempts(attempts, selectedSubscription.queries),
          issueNextCursor: issues ? issues.next_cursor : current.issueNextCursor,
          failedPostNextCursor: failed ? failed.nextCursor : current.failedPostNextCursor,
          issueTotalCount: issues?.total_count ?? current.issueTotalCount,
          failedPostTotalCount: failed?.totalCount ?? current.failedPostTotalCount,
          retryablePostCount: failed?.retryableCount ?? current.retryablePostCount,
        };
      });
    }, { refresh: false });
  }, [act, busyKey, detail.failedPostNextCursor, detail.issueNextCursor, selectedSubscription, setDetail]);

  /** User-initiated navigation inside the workspace — recorded in app history. */
  const navigateTo = useCallback((next: typeof selection) => {
    setSelection(next);
    pushSubscriptionsHistory(next);
  }, [setSelection]);

  const createFromWizard = useCallback(async (result: CreateSubscriptionInput) => {
    await act('wizard', async () => {
      const subscription = await subscriptionsController.create({
        name: result.name,
        initial_post_limit: result.initialPostLimit,
        periodic_post_limit: result.periodicPostLimit,
      });
      navigateTo({ kind: 'subscription', id: subscription.id });
      setWizard({ open: false });
    });
  }, [act, navigateTo, setWizard]);

  const busy = busyKey != null;
  const detailController = {
    run: (id: string) => {
      markSubscriptionRunTriggered();
      void act(`run:${id}`, () => subscriptionsController.run(id));
    },
    stop: (id: string) => void act(`stop:${id}`, () => subscriptionsController.stop(id)),
    pause: (id: string, paused: boolean) => void act(`pause:${id}`, () => subscriptionsController.pause(id, paused)),
    reset: (id: string) => {
      confirm(
        {
          title: 'Reset Sync Progress',
          message: 'Sync progress and download history for this subscription will be cleared. The next run re-downloads everything.',
          confirmLabel: 'Reset',
        },
        () => void act(`reset:${id}`, () => subscriptionsController.reset(id)),
      );
    },
    delete: (id: string) => {
      confirm(
        {
          title: 'Delete Subscription',
          message: 'Downloaded files stay in your library. The subscription and its queries are removed.',
          confirmLabel: 'Delete',
          danger: true,
        },
        () => void act(`delete:${id}`, () => subscriptionsController.delete(id)),
      );
    },
    rename: (id: string, currentName: string) => {
      setRenameTarget({ kind: 'subscription', id, currentName });
    },
    setSchedule: (id: string, schedule: string) =>
      void act(`schedule:${id}`, () => subscriptionsController.setSchedule(id, schedule)),
    runQuery: (subscriptionId: string, queryId: string) => {
      markSubscriptionRunTriggered();
      void act(`runq:${queryId}`, () => subscriptionsController.runQuery(subscriptionId, queryId));
    },
    stopQuery: (subscriptionId: string, queryId: string) =>
      void act(`stopq:${queryId}`, () => subscriptionsController.stopQuery(subscriptionId, queryId)),
    pauseQuery: (queryId: string, paused: boolean) =>
      void act(`pauseq:${queryId}`, () => subscriptionsController.pauseQuery(queryId, paused)),
    deleteQuery: (queryId: string) => {
      confirm(
        { title: 'Delete Query', message: 'This query stops syncing. Downloaded files stay in your library.', confirmLabel: 'Delete', danger: true },
        () => void act(`delq:${queryId}`, () => subscriptionsController.deleteQuery(queryId)),
      );
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

  /** Right-click menu for one subscription card or detail overflow button. */
  const openSubscriptionMenu = useCallback(
    (position: { x: number; y: number }, subscription: SubscriptionInfo) => {
      if (!snapshot) return;
      const running = snapshot.runningSubscriptionIds.includes(subscription.id)
        || progressBySubscriptionId.has(subscription.id);
      contextMenu.openAt(position, buildSubscriptionMenu({
        subscription,
        running,
        onRun: () => detailController.run(subscription.id),
        onStop: () => detailController.stop(subscription.id),
        onPause: (paused) => detailController.pause(subscription.id, paused),
        onRename: () => setRenameTarget({ kind: 'subscription', id: subscription.id, currentName: subscription.name }),
        onSetSchedule: (schedule) => detailController.setSchedule(subscription.id, schedule),
        onReset: () => detailController.reset(subscription.id),
        onDelete: () => detailController.delete(subscription.id),
      }));
    },
    [contextMenu, snapshot, progressBySubscriptionId, detailController],
  );

  /** Bulk menu for a multi-card selection on the subscriptions grid. */
  const openMultiCardMenu = useCallback(
    (position: { x: number; y: number }, subscriptionIds: string[]) => {
      if (!snapshot) return;
      const anyRunning = subscriptionIds.some(
        (id) => snapshot.runningSubscriptionIds.includes(id) || progressBySubscriptionId.has(id),
      );
      contextMenu.openAt(position, buildMultiCardMenu({
        subscriptionIds,
        anyRunning,
        onRunSelected: () => {
          markSubscriptionRunTriggered();
          void act('multi:run', async () => {
            for (const sid of subscriptionIds) await subscriptionsController.run(sid).catch(() => {});
          });
        },
        onPauseSelected: (paused) =>
          void act('multi:pause', async () => {
            for (const sid of subscriptionIds) await subscriptionsController.pause(sid, paused).catch(() => {});
          }),
        onDeleteSelected: () => {
          const total = subscriptionIds.length;
          confirm(
            {
              title: `Delete ${total} Item${total === 1 ? '' : 's'}`,
              message: 'Downloaded files stay in your library. The selected subscriptions and their queries are removed.',
              confirmLabel: 'Delete',
              danger: true,
            },
            () => void act('multi:delete', async () => {
              for (const sid of subscriptionIds) await subscriptionsController.delete(sid);
            }),
          );
        },
      }));
    },
    [contextMenu, snapshot, progressBySubscriptionId, act, confirm],
  );

  const commitRename = useCallback(
    (target: RenameTarget, name: string) => {
      const action = () => subscriptionsController.rename(target.id, name);
      void act(`rename:${target.kind}:${target.id}`, action).then(() => setRenameTarget(null));
    },
    [act],
  );

  return (
    <div className={styles.root}>
      <main className={styles.detailPane}>
        {error && (
          <div className={styles.errorBanner}>
            <span>{error}</span>
            <button
              type="button"
              className={styles.errorBannerDismiss}
              onClick={() => setError(null)}
              aria-label="Dismiss error"
            >
              ×
            </button>
          </div>
        )}
        {loading && !snapshot ? (
          <EmptyState title="Loading…" description="Fetching subscriptions." />
        ) : selection == null && snapshot ? (
          <SubscriptionsGrid
            subscriptions={snapshot.subscriptions}
            listMetrics={snapshot.listMetrics}
            covers={covers}
            progressBySubscriptionId={progressBySubscriptionId}
            runningSubscriptionIds={snapshot.runningSubscriptionIds}
            onSelect={navigateTo}
            onAdd={() => setWizard({ open: true })}
            onOpenAccounts={() => setAccountsModal({ open: true, focusSiteId: null })}
            onSubscriptionMenu={(position, id) => {
              const subscription = snapshot.subscriptions.find((sub) => sub.id === id);
              if (subscription) openSubscriptionMenu(position, subscription);
            }}
            onMultiMenu={openMultiCardMenu}
          />
        ) : selectedSubscription && snapshot ? (
          <SubscriptionDetail
            subscription={selectedSubscription}
            snapshot={snapshot}
            progress={selectedProgress}
            detail={detail}
            coverHash={covers.get(selectedSubscription.id) ?? null}
            activeTab={activeTab}
            busy={busy}
            controller={{
              ...detailController,
              retryFailedPosts: () => {
                void act('retryposts', async () => {
                  await subscriptionsController.retryFailedPosts(selectedSubscription.id);
                  await refreshDetail(selectedSubscription);
                });
              },
              retryFailedPost: (post) => {
                if (!post.queryId) return;
                void act(`retrypost:${post.key}`, async () => {
                  await subscriptionsController.retryFailedPost({
                    subscription_id: selectedSubscription.id,
                    query_id: post.queryId as string,
                    post_id: post.postId,
                  });
                  await refreshDetail(selectedSubscription);
                });
              },
            }}
            onTabChange={setActiveTab}
            onOpenAccounts={(siteId) => setAccountsModal({
              open: true,
              focusSiteId: siteId ? getCredentialOwnerSiteId(siteId, snapshot.sites) : null,
            })}
            onLoadMoreHealth={() => void loadMoreHealth()}
            onOpenMenu={(position) => openSubscriptionMenu(position, selectedSubscription)}
          />
        ) : (
          <EmptyState
            title="Subscribe to artists and tags"
            description="Create a subscription and new posts will land in your library automatically."
          />
        )}
      </main>

      <NewSubscriptionDialog
        open={wizard.open}
        busy={busy}
        onCreate={(result) => void createFromWizard(result)}
        onClose={() => setWizard({ open: false })}
      />

      <AccountsModal
        open={accountsModal.open}
        focusSiteId={accountsModal.focusSiteId}
        onClose={() => {
          setAccountsModal({ open: false, focusSiteId: null });
          void refreshSubscriptionsWorkspace();
        }}
      />

      <RenameDialog
        target={renameTarget}
        busy={busy}
        onRename={commitRename}
        onClose={() => setRenameTarget(null)}
      />

      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
        />
      )}
    </div>
  );
}
