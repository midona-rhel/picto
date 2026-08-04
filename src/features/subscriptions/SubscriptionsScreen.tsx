import { useCallback, useEffect, useState } from 'react';
import { pushSubscriptionsHistory } from '../../state/navigationHistory';
import { useAtom, useAtomValue, useSetAtom } from 'jotai';
import { confirmModalAtom } from '../../state/modals';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { subscriptionsController } from '../../controllers/subscriptionsController';
import {
  buildGroupMenu,
  buildMultiCardMenu,
  buildSubscriptionMenu,
} from './subscriptionsContextMenu';
import { RenameDialog, type RenameTarget } from './components/RenameDialog';
import {
  markSubscriptionRunTriggered,
  refreshSubscriptionsWorkspace,
} from '../../runtime/subscriptionsSettle';
import type { SubscriptionInfo } from '../../shared/types/subscriptions';
import { AccountsModal } from './components/AccountsModal';
import { SubscriptionsGrid } from './components/SubscriptionsGrid';
import { GroupDetail } from './components/GroupDetail';
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
  const selectedGroup = useAtomValue(subscriptionsSelectedGroupAtom);
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
      setDetail({ loading: false, error: null, subscriptionId: subscription.id, runs, issues, failedPosts });
    } catch (err) {
      setDetail({
        ...EMPTY_SUBSCRIPTION_DETAIL_STATE,
        error: err instanceof Error ? err.message : String(err),
        subscriptionId: subscription.id,
      });
    }
  }, [setDetail]);

  // Runtime owns backend settlement; the screen only requests its initial snapshot.
  useEffect(() => {
    void refreshSubscriptionsWorkspace();
  }, []);

  // Detail follows the selected subscription
  useEffect(() => {
    if (selectedSubscription) void refreshDetail(selectedSubscription);
  }, [selectedSubscription?.id, refreshDetail]); // eslint-disable-line react-hooks/exhaustive-deps

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

  /** User-initiated navigation inside the workspace — recorded in app history. */
  const navigateTo = useCallback((next: typeof selection) => {
    setSelection(next);
    pushSubscriptionsHistory(next);
  }, [setSelection]);

  const createFromWizard = useCallback(async (result: CreateSubscriptionInput) => {
    await act('wizard', async () => {
      const subscription = await subscriptionsController.create({
        name: result.name,
        group_id: null,
        initial_post_limit: result.initialPostLimit,
        periodic_post_limit: result.periodicPostLimit,
      });
      if (!result.autoCollections) {
        await subscriptionsController.setAutoCollections(subscription.id, false);
      }
      await subscriptionsController.addQuery(subscription.id, result.siteId, result.queryText);
      if (result.runNow) {
        markSubscriptionRunTriggered();
        await subscriptionsController.run(subscription.id);
      }
      navigateTo({ kind: 'subscription', id: subscription.id });
      setWizard({ open: false, initialSiteId: null });
    });
  }, [act, setSelection, setWizard]);

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
    setAutoCollections: (id: string, on: boolean) =>
      void act(`autocol:${id}`, () => subscriptionsController.setAutoCollections(id, on)),
    setSchedule: (id: string, schedule: string) =>
      void act(`schedule:${id}`, () => subscriptionsController.setSchedule(id, schedule)),
    setGroup: (id: string, groupId: number | null) =>
      void act(`setgroup:${id}`, () => subscriptionsController.setSubscriptionGroup(id, groupId)),
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

  /** Right-click menu for one subscription (card, sidebar row, or detail ⋮). */
  const openSubscriptionMenu = useCallback(
    (position: { x: number; y: number }, subscription: SubscriptionInfo) => {
      if (!snapshot) return;
      const running = snapshot.runningSubscriptionIds.includes(subscription.id)
        || progressBySubscriptionId.has(subscription.id);
      contextMenu.openAt(position, buildSubscriptionMenu({
        subscription,
        running,
        groups: snapshot.groups,
        onRun: () => detailController.run(subscription.id),
        onStop: () => detailController.stop(subscription.id),
        onPause: (paused) => detailController.pause(subscription.id, paused),
        onRename: () => setRenameTarget({ kind: 'subscription', id: subscription.id, currentName: subscription.name }),
        onSetSchedule: (schedule) => detailController.setSchedule(subscription.id, schedule),
        onMoveToGroup: (groupId) => detailController.setGroup(subscription.id, groupId),
        onToggleAutoCollections: () =>
          detailController.setAutoCollections(subscription.id, !subscription.auto_collections),
        onReset: () => detailController.reset(subscription.id),
        onDelete: () => detailController.delete(subscription.id),
      }));
    },
    [contextMenu, snapshot, progressBySubscriptionId, detailController],
  );

  const openGroupMenu = useCallback(
    (position: { x: number; y: number }, groupId: string) => {
      const group = snapshot?.groups.find((entry) => entry.id === groupId);
      if (!group || !snapshot) return;
      const anyRunning = group.subscriptions.some(
        (sub) => snapshot.runningSubscriptionIds.includes(sub.id) || progressBySubscriptionId.has(sub.id),
      );
      contextMenu.openAt(position, buildGroupMenu({
        group,
        anyRunning,
        onRunAll: () => {
          markSubscriptionRunTriggered();
          void act(`rungroup:${group.id}`, () => subscriptionsController.runGroup(group.id));
        },
        onStopAll: () => void act(`stopgroup:${group.id}`, () => subscriptionsController.stopGroup(group.id)),
        onRename: () => setRenameTarget({ kind: 'group', id: group.id, currentName: group.name }),
        onDelete: () => {
          confirm(
            { title: 'Delete Group', message: 'Its subscriptions are kept and become ungrouped.', confirmLabel: 'Delete', danger: true },
            () => void act(`delgroup:${group.id}`, async () => {
              await subscriptionsController.deleteGroup(group.id);
              if (selection?.kind === 'group' && selection.id === group.id) navigateTo(null);
            }),
          );
        },
      }));
    },
    [contextMenu, snapshot, progressBySubscriptionId, act, confirm, navigateTo, selection],
  );

  /** Bulk menu for a multi-card selection on the subscriptions grid. */
  const openMultiCardMenu = useCallback(
    (position: { x: number; y: number }, subscriptionIds: string[], groupIds: string[]) => {
      if (!snapshot) return;
      const allSubIds = [
        ...subscriptionIds,
        ...groupIds.flatMap((gid) =>
          snapshot.groups.find((g) => g.id === gid)?.subscriptions.map((s) => s.id) ?? []),
      ];
      contextMenu.openAt(position, buildMultiCardMenu({
        subscriptionIds,
        groupIds,
        groups: snapshot.groups,
        onRunSelected: () => {
          markSubscriptionRunTriggered();
          void act('multi:run', async () => {
            for (const gid of groupIds) await subscriptionsController.runGroup(gid).catch(() => {});
            for (const sid of subscriptionIds) await subscriptionsController.run(sid).catch(() => {});
          });
        },
        onPauseSelected: (paused) =>
          void act('multi:pause', async () => {
            for (const sid of subscriptionIds) await subscriptionsController.pause(sid, paused).catch(() => {});
          }),
        onMoveSelectedToGroup: (groupId) =>
          void act('multi:move', async () => {
            for (const sid of subscriptionIds) await subscriptionsController.setSubscriptionGroup(sid, groupId);
          }),
        onDeleteSelected: () => {
          const total = subscriptionIds.length + groupIds.length;
          confirm(
            {
              title: `Delete ${total} Item${total === 1 ? '' : 's'}`,
              message: 'Downloaded files stay in your library. Groups are removed; their subscriptions become ungrouped.',
              confirmLabel: 'Delete',
              danger: true,
            },
            () => void act('multi:delete', async () => {
              for (const sid of subscriptionIds) await subscriptionsController.delete(sid);
              for (const gid of groupIds) await subscriptionsController.deleteGroup(gid);
            }),
          );
        },
      }));
      void allSubIds;
    },
    [contextMenu, snapshot, act, confirm],
  );

  const commitRename = useCallback(
    (target: RenameTarget, name: string) => {
      const action = target.kind === 'group'
        ? () => subscriptionsController.renameGroup(target.id, name)
        : () => subscriptionsController.rename(target.id, name);
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
            groups={snapshot.groups}
            subscriptions={snapshot.subscriptions}
            listMetrics={snapshot.listMetrics}
            covers={covers}
            progressBySubscriptionId={progressBySubscriptionId}
            runningSubscriptionIds={snapshot.runningSubscriptionIds}
            onSelect={navigateTo}
            onFollow={() => setWizard({ open: true, initialSiteId: null })}
            onOpenAccounts={() => setAccountsModal({ open: true, focusSiteId: null })}
            onSubscriptionMenu={(position, id) => {
              const subscription = snapshot.subscriptions.find((sub) => sub.id === id);
              if (subscription) openSubscriptionMenu(position, subscription);
            }}
            onGroupMenu={openGroupMenu}
            onMultiMenu={openMultiCardMenu}
          />
        ) : selectedGroup && snapshot ? (
          <GroupDetail
            group={selectedGroup}
            sites={snapshot.sites}
            runningSubscriptionIds={snapshot.runningSubscriptionIds}
            coverHash={
              selectedGroup.subscriptions
                .filter((sub) => covers.has(sub.id))
                .sort((a, b) => Number(b.id) - Number(a.id))
                .map((sub) => covers.get(sub.id))[0] ?? null
            }
            busy={busy}
            onRename={(name) => void act(`renamegroup:${selectedGroup.id}`, () => subscriptionsController.renameGroup(selectedGroup.id, name))}
            onRun={() => {
              markSubscriptionRunTriggered();
              void act(`rungroup:${selectedGroup.id}`, () => subscriptionsController.runGroup(selectedGroup.id));
            }}
            onStop={() => void act(`stopgroup:${selectedGroup.id}`, () => subscriptionsController.stopGroup(selectedGroup.id))}
            onDelete={() => {
              confirm(
                { title: 'Delete Group', message: 'Its subscriptions are kept and become ungrouped.', confirmLabel: 'Delete', danger: true },
                () => void act(`delgroup:${selectedGroup.id}`, async () => {
                  await subscriptionsController.deleteGroup(selectedGroup.id);
                  navigateTo(null);
                }),
              );
            }}
            onAddSource={async (siteId, queryText) => {
              await act(`groupsource:${selectedGroup.id}`, async () => {
                const siteName = snapshot.sites.find((site) => site.id === siteId)?.name ?? siteId;
                const subscription = await subscriptionsController.create({
                  name: `${siteName}: ${queryText}`,
                  group_id: Number.parseInt(selectedGroup.id, 10),
                  initial_post_limit: 100,
                  periodic_post_limit: 50,
                });
                await subscriptionsController.addQuery(subscription.id, siteId, queryText);
              });
            }}
            onRemoveSubscription={(subscriptionId) =>
              void act(`groupdel:${subscriptionId}`, () =>
                subscriptionsController.setSubscriptionGroup(subscriptionId, null))}
            onSelectSubscription={(subscriptionId) => navigateTo({ kind: 'subscription', id: subscriptionId })}
          />
        ) : selectedSubscription && snapshot ? (
          <SubscriptionDetail
            subscription={selectedSubscription}
            snapshot={snapshot}
            groups={snapshot.groups}
            progress={selectedProgress}
            detail={detail}
            coverHash={covers.get(selectedSubscription.id) ?? null}
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
        sites={snapshot?.sites ?? []}
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
          searchable={false}
        />
      )}
    </div>
  );
}
