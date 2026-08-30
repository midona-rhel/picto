import { useCallback, useEffect, useRef, useState } from 'react';
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
} from '../../runtime/subscriptionsSettle';
import type { SubscriptionInfo } from '../../shared/types/subscriptions';
import { getCredentialOwnerSiteId } from '../../shared/lib/subscriptionHelpers';
import { showErrorNotification } from '../../shared/lib/notifications';
import { AccountsModal } from './components/AccountsModal';
import { SubscriptionsGrid } from './components/SubscriptionsGrid';
import { SubscriptionDetail } from './components/SubscriptionDetail';
import { SubscriptionCoverDialog } from './components/SubscriptionCoverDialog';
import { EmptyState } from './components/EmptyState';
import { NewSubscriptionDialog, type CreateSubscriptionInput } from './components/NewSubscriptionDialog';
import { AddGalleryDialog, type AddGalleryInput } from './components/AddGalleryDialog';
import { isGalleryImportJob, isVisibleGalleryImportJob } from './subscriptionUtils';
import { createSubscriptionActionQueue } from './subscriptionActionQueue';
import {
  EMPTY_SUBSCRIPTION_DETAIL_STATE,
  beginSubscriptionDetailRefresh,
  subscriptionsAccountsModalAtom,
  subscriptionsBusyKeyAtom,
  subscriptionsCoversAtom,
  subscriptionsDetailAtom,
  subscriptionsProgressBySubscriptionIdAtom,
  subscriptionsSelectedProgressAtom,
  subscriptionsSelectedSubscriptionAtom,
  subscriptionsSelectionAtom,
  subscriptionsWizardAtom,
  subscriptionsWorkspaceSnapshotAtom,
} from '../../state/subscriptionsWorkspace';
import styles from './SubscriptionsScreen.module.css';

export function SubscriptionsScreen() {
  const [snapshot, setSnapshot] = useAtom(subscriptionsWorkspaceSnapshotAtom);
  const [selection, setSelection] = useAtom(subscriptionsSelectionAtom);
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
  const [coverTarget, setCoverTarget] = useState<{ id: string; name: string } | null>(null);
  const [galleryDialogOpen, setGalleryDialogOpen] = useState(false);
  const actionQueue = useRef(createSubscriptionActionQueue());

  const confirm = useCallback(
    (opts: { title: string; message: string; confirmLabel?: string; danger?: boolean }, action: () => void) => {
      setConfirmModal({ ...opts, open: true, onConfirm: action });
    },
    [setConfirmModal],
  );

  const refreshDetail = useCallback(async (subscription: SubscriptionInfo) => {
    // Revalidation must not replace an already painted detail view with its
    // loading state. Only a genuine subscription change needs that placeholder.
    setDetail((current) => beginSubscriptionDetailRefresh(current, subscription.id));
    try {
      const [runs, issues] = await Promise.all([
        subscriptionsController.listRuns(subscription.id),
        subscriptionsController.listIssues(subscription.id),
      ]);
      await subscriptionsController.acknowledgeIssues(subscription.id);
      setSnapshot((current) => {
        if (!current) return current;
        const metrics = current.listMetrics[subscription.id];
        if (!metrics || metrics.openIssueCount === 0) return current;
        return {
          ...current,
          listMetrics: {
            ...current.listMetrics,
            [subscription.id]: { ...metrics, openIssueCount: 0 },
          },
        };
      });
      setDetail({
        loading: false,
        subscriptionId: subscription.id,
        runs,
        issues: issues.items,
        failedPosts: [],
        attempts: [],
        issueNextCursor: issues.next_cursor,
        failedPostNextCursor: null,
        issueTotalCount: issues.total_count,
        failedPostTotalCount: 0,
        retryablePostCount: 0,
      });
    } catch (err) {
      setDetail({
        ...EMPTY_SUBSCRIPTION_DETAIL_STATE,
        subscriptionId: subscription.id,
      });
      showErrorNotification({
        title: 'Subscription details unavailable',
        message: err instanceof Error ? err.message : String(err),
      });
    }
  }, [setDetail, setSnapshot]);

  // Detail follows the selected subscription
  useEffect(() => {
    if (selectedSubscription) void refreshDetail(selectedSubscription);
  }, [selectedSubscription, refreshDetail]);

  // A menu belongs to the surface it was opened on — close it on navigation.
  useEffect(() => {
    contextMenu.close();
  }, [selection?.kind, selection?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const act = useCallback((key: string, action: () => Promise<unknown>, options?: { refresh?: boolean }) => {
    return actionQueue.current(async () => {
      setBusyKey(key);
      try {
        await action();
        if (options?.refresh !== false) await refreshSubscriptionsWorkspace();
        return true;
      } catch (err) {
        showErrorNotification({
          title: 'Subscription action failed',
          message: err instanceof Error ? err.message : String(err),
        });
        return false;
      } finally {
        setBusyKey(null);
      }
    });
  }, [setBusyKey]);

  const loadMoreHealth = useCallback(async () => {
    if (!selectedSubscription || busyKey) return;
    const issueCursor = detail.issueNextCursor;
    if (issueCursor == null) return;
    await act('health:more', async () => {
      const issues = await subscriptionsController.listIssues(selectedSubscription.id, issueCursor);
      setDetail((current) => {
        return {
          ...current,
          issues: issues ? [...current.issues, ...issues.items] : current.issues,
          issueNextCursor: issues ? issues.next_cursor : current.issueNextCursor,
          issueTotalCount: issues?.total_count ?? current.issueTotalCount,
        };
      });
    }, { refresh: false });
  }, [act, busyKey, detail.issueNextCursor, selectedSubscription, setDetail]);

  /** User-initiated navigation inside the workspace — recorded in app history. */
  const navigateTo = useCallback((next: typeof selection) => {
    setSelection(next);
    pushSubscriptionsHistory(next);
  }, [setSelection]);

  const createFromWizard = useCallback(async (result: CreateSubscriptionInput) => {
    await act('wizard', async () => {
      const subscription = await subscriptionsController.create({
        name: result.name,
      });
      navigateTo({ kind: 'subscription', id: subscription.id });
      setWizard({ open: false });
    });
  }, [act, navigateTo, setWizard]);

  const addGallery = useCallback(async (result: AddGalleryInput) => {
    const succeeded = await act('gallery', async () => {
      markSubscriptionRunTriggered();
      await subscriptionsController.startGalleryImport(result.url, result.serviceId);
    });
    if (succeeded) setGalleryDialogOpen(false);
  }, [act]);

  const busy = busyKey != null;
  const galleryJobs = snapshot?.subscriptions.filter(isVisibleGalleryImportJob) ?? [];
  const galleryImportRunning = galleryJobs.some(
    (job) => snapshot?.runningSubscriptionIds.includes(job.id) || progressBySubscriptionId.has(job.id),
  );
  const visibleSubscriptions = snapshot?.subscriptions.filter(
    (subscription) => !isGalleryImportJob(subscription),
  ) ?? [];
  const detailController = {
    run: (id: string) => {
      markSubscriptionRunTriggered();
      void act(`run:${id}`, () => subscriptionsController.run(id));
    },
    stop: (id: string) => void act(`stop:${id}`, () => subscriptionsController.stop(id)),
    pause: (id: string, paused: boolean) => void act(`pause:${id}`, () => subscriptionsController.pause(id, paused)),
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
    reset: (id: string) => {
      confirm(
        {
          title: 'Reset Sync History',
          message: 'Picto will clear this subscription\'s handled-file history and scan every query from the beginning. Existing media and account login remain untouched; previously deleted source media can be downloaded again.',
          confirmLabel: 'Reset',
        },
        () => void act(`reset:${id}`, () => subscriptionsController.reset(id)),
      );
    },
    rename: (id: string, currentName: string) => {
      setRenameTarget({ kind: 'subscription', id, currentName });
    },
    setSchedule: (id: string, schedule: string) =>
      void act(`schedule:${id}`, () => subscriptionsController.setSchedule(id, schedule)),
    setPostsPerRun: (id: string, postsPerRun: number) =>
      void act(`posts-per-run:${id}`, () => subscriptionsController.setPostsPerRun(id, postsPerRun)),
    setDestination: (id: string, destination: { target_folder_ids: number[]; automatic_tags: string[] }) =>
      act(`destination:${id}`, () => subscriptionsController.setDestination(id, destination)).then(() => undefined),
    pauseQuery: (queryId: string, paused: boolean) =>
      void act(`pauseq:${queryId}`, () => subscriptionsController.pauseQuery(queryId, paused)),
    runQuery: (queryId: string) => {
      markSubscriptionRunTriggered();
      void act(`runq:${queryId}`, () => subscriptionsController.runQuery(queryId));
    },
    setQueryGrouping: (queryId: string, groupPosts: boolean) =>
      void act(`groupq:${queryId}`, () => subscriptionsController.setQueryGrouping(queryId, groupPosts)),
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

  const deleteSubscriptions = useCallback((subscriptionIds: string[]) => {
    const total = subscriptionIds.length;
    confirm(
      {
        title: `Delete ${total} Subscription${total === 1 ? '' : 's'}`,
        message: 'Downloaded files stay in your library. The selected subscriptions and their queries are removed.',
        confirmLabel: 'Delete',
        danger: true,
      },
      () => void act('multi:delete', async () => {
        for (const id of subscriptionIds) await subscriptionsController.delete(id);
      }),
    );
  }, [act, confirm]);

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
        onSetCover: () => setCoverTarget({ id: subscription.id, name: subscription.name }),
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
      const selectedSubscriptions = subscriptionIds
        .map((id) => snapshot.subscriptions.find((subscription) => subscription.id === id))
        .filter((subscription): subscription is SubscriptionInfo => subscription != null);
      contextMenu.openAt(position, buildMultiCardMenu({
        subscriptionIds,
        schedules: selectedSubscriptions.map((subscription) => subscription.schedule),
        anyRunning,
        onRunSelected: () => {
          markSubscriptionRunTriggered();
          void act('multi:run', async () => {
            for (const id of subscriptionIds) await subscriptionsController.run(id);
          });
        },
        onPauseSelected: (paused) =>
          void act('multi:pause', async () => {
            for (const id of subscriptionIds) await subscriptionsController.pause(id, paused);
          }),
        onSetScheduleSelected: (schedule) =>
          void act('multi:schedule', async () => {
            for (const id of subscriptionIds) await subscriptionsController.setSchedule(id, schedule);
          }),
        onDeleteSelected: () => deleteSubscriptions(subscriptionIds),
      }));
    },
    [contextMenu, snapshot, progressBySubscriptionId, act, deleteSubscriptions],
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
        {selection == null && snapshot ? (
          <SubscriptionsGrid
            subscriptions={visibleSubscriptions}
            galleryJobs={galleryJobs}
            listMetrics={snapshot.listMetrics}
            covers={covers}
            progressBySubscriptionId={progressBySubscriptionId}
            runningSubscriptionIds={snapshot.runningSubscriptionIds}
            onSelect={navigateTo}
            onAdd={() => setWizard({ open: true })}
            galleryImportRunning={galleryImportRunning}
            onAddGallery={() => setGalleryDialogOpen(true)}
            onOpenAccounts={() => setAccountsModal({ open: true, focusSiteId: null })}
            onSubscriptionMenu={(position, id) => {
              const subscription = snapshot.subscriptions.find((sub) => sub.id === id);
              if (subscription) openSubscriptionMenu(position, subscription);
            }}
            onMultiMenu={openMultiCardMenu}
            onDeleteSelected={deleteSubscriptions}
          />
        ) : selectedSubscription && snapshot ? (
          <SubscriptionDetail
            subscription={selectedSubscription}
            snapshot={snapshot}
            progress={selectedProgress}
            detail={detail}
            cover={covers.get(selectedSubscription.id) ?? null}
            busy={busy}
            controller={{
              ...detailController,
            }}
            onOpenAccounts={(siteId) => setAccountsModal({
              open: true,
              focusSiteId: siteId ? getCredentialOwnerSiteId(siteId, snapshot.sites) : null,
            })}
            onLoadMoreHealth={() => void loadMoreHealth()}
            onOpenMenu={(position) => openSubscriptionMenu(position, selectedSubscription)}
          />
        ) : snapshot ? (
          <EmptyState
            title="Subscribe to artists and tags"
            description="Create a subscription and new posts will land in your library automatically."
          />
        ) : null}
      </main>

      <NewSubscriptionDialog
        open={wizard.open}
        busy={busy}
        onCreate={(result) => void createFromWizard(result)}
        onClose={() => setWizard({ open: false })}
      />

      <AddGalleryDialog
        open={galleryDialogOpen}
        busy={busy || galleryImportRunning}
        onAdd={(result) => void addGallery(result)}
        onClose={() => setGalleryDialogOpen(false)}
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

      <SubscriptionCoverDialog
        target={coverTarget}
        busy={busy}
        onLoad={(id, cursor) => subscriptionsController.getCoverCandidates(id, cursor)}
        onSave={(id, candidate, crop) => act(`cover:${id}`, () => subscriptionsController.setCover(id, {
          media_item_id: candidate.media_item_id,
          focus_x: crop.focusX,
          focus_y: crop.focusY,
          zoom_percent: crop.zoomPercent,
        }))}
        onClose={() => setCoverTarget(null)}
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
