import { useCallback, useEffect, useRef, useState } from 'react';
import { IconAlertTriangle, IconCheck, IconCloud, IconDownload } from '@tabler/icons-react';
import type { UnlistenFn } from '#desktop/api';

import {
  PtrSyncController,
  type PtrBootstrapStatus,
  type PtrSyncProgress,
  type PtrSyncResult,
} from '../../controllers/ptrSyncController';
import {
  SubscriptionController,
  type SubscriptionProgressEvent,
  type SubscriptionStartedEvent,
  type SubscriptionFinishedEvent,
} from '../../controllers/subscriptionController';
import { SidebarController } from '../../controllers/sidebarController';
import { logBestEffortError } from '../../lib/asyncOps';
import { notifyError, notifySuccess } from '../../lib/notify';
import st from './SidebarJobStatus.module.css';

interface SubProgress {
  subscription_id: string;
  subscription_name: string;
  query_name?: string;
  files_downloaded: number;
  files_skipped: number;
  pages_fetched: number;
  status_text: string;
}

type PtrState =
  | { phase: 'idle' }
  | { phase: 'syncing'; progress: PtrSyncProgress | null }
  | { phase: 'done'; success: true }
  | { phase: 'done'; success: false; error: string };

type PtrBootstrapState =
  | { phase: 'idle' }
  | { phase: 'running'; status: PtrBootstrapStatus }
  | { phase: 'done'; success: true }
  | { phase: 'done'; success: false; error: string };

export function SidebarJobStatus() {
  const [ptrState, setPtrState] = useState<PtrState>({ phase: 'idle' });
  const [ptrBootstrapState, setPtrBootstrapState] = useState<PtrBootstrapState>({ phase: 'idle' });
  const [subs, setSubs] = useState<Map<string, SubProgress>>(new Map());
  const subNameByIdRef = useRef<Map<string, string>>(new Map());
  const subFinishTimersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  const fadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const bootstrapFadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // PBI-039: Track last event timestamp for watchdog gating.
  const lastEventRef = useRef<number>(Date.now());

  const syncRunningSubscriptions = useCallback(async (refreshNames = false): Promise<number> => {
    try {
      const [ids, snapshots] = await Promise.all([
        SubscriptionController.getRunningSubscriptions(),
        SubscriptionController.getRunningSubscriptionProgress().catch((error) => {
          logBestEffortError('sidebarJobStatus.getRunningSubscriptionProgress', error);
          return [];
        }),
      ]);

      let nameById = subNameByIdRef.current;
      if (refreshNames || nameById.size === 0) {
        const subscriptions = await SubscriptionController.getSubscriptions<{
          id: string;
          name: string;
        }>().catch((error) => {
          logBestEffortError('sidebarJobStatus.getSubscriptions', error);
          return [];
        });
        nameById = new Map<string, string>();
        for (const sub of subscriptions) {
          const name = (sub.name ?? '').trim();
          if (name) nameById.set(sub.id, name);
        }
        subNameByIdRef.current = nameById;
      }
      const snapshotById = new Map(snapshots.map((snap) => [snap.subscription_id, snap]));
      const activeIds = new Set<string>([...ids, ...snapshotById.keys()]);

      setSubs((prev) => {
        const next = new Map(prev);

        for (const id of Array.from(next.keys())) {
          if (!activeIds.has(id)) next.delete(id);
        }

        for (const id of activeIds) {
          const snapshot = snapshotById.get(id);
          if (!next.has(id)) {
            const resolvedName =
              (snapshot?.subscription_name ?? '').trim() ||
              (nameById.get(id) ??
                prev.get(id)?.subscription_name ??
                `Subscription ${id}`);
            next.set(id, {
              subscription_id: id,
              subscription_name: resolvedName,
              query_name: snapshot?.query_name,
              files_downloaded: snapshot?.files_downloaded ?? 0,
              files_skipped: snapshot?.files_skipped ?? 0,
              pages_fetched: snapshot?.pages_fetched ?? 0,
              status_text: snapshot?.status_text ?? 'Running...',
            });
          } else if (snapshot) {
            const existing = next.get(id);
            const resolvedName =
              (snapshot.subscription_name ?? '').trim() ||
              existing?.subscription_name ||
              nameById.get(id) ||
              `Subscription ${id}`;
            next.set(id, {
              subscription_id: id,
              subscription_name: resolvedName,
              query_name: snapshot.query_name ?? existing?.query_name,
              files_downloaded: snapshot.files_downloaded ?? existing?.files_downloaded ?? 0,
              files_skipped: snapshot.files_skipped ?? existing?.files_skipped ?? 0,
              pages_fetched: snapshot.pages_fetched ?? existing?.pages_fetched ?? 0,
              status_text: snapshot.status_text ?? existing?.status_text ?? 'Running...',
            });
          }
        }
        return next;
      });
      return activeIds.size;
    } catch (error) {
      logBestEffortError('sidebarJobStatus.syncRunningSubscriptions', error);
      return 0;
    }
  }, []);

  useEffect(() => {
    const cleanups: UnlistenFn[] = [];
    let disposed = false;
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let subProgressPollTimer: ReturnType<typeof setInterval> | null = null;
    const push = (fn: UnlistenFn) => { if (disposed) fn(); else cleanups.push(fn); };

    // Check initial state
    PtrSyncController.isSyncing().then((running) => {
      if (running) setPtrState({ phase: 'syncing', progress: null });
    }).catch((error) => {
      logBestEffortError('sidebarJobStatus.ptrIsSyncing', error);
    });
    PtrSyncController.getBootstrapStatus().then((status) => {
      if (status.running) {
        setPtrBootstrapState({ phase: 'running', status });
      }
    }).catch((error) => {
      logBestEffortError('sidebarJobStatus.ptrBootstrapStatus', error);
    });

    void syncRunningSubscriptions(true).then((runningCount) => {
      if (runningCount > 0) SidebarController.requestRefresh();
    });

    const setup = async () => {
      push(await PtrSyncController.onStarted(() => {
        lastEventRef.current = Date.now();
        if (fadeTimerRef.current) clearTimeout(fadeTimerRef.current);
        setPtrState({ phase: 'syncing', progress: null });
      }));
      push(await PtrSyncController.onProgress((progress) => {
        lastEventRef.current = Date.now();
        setPtrState({ phase: 'syncing', progress });
      }));
      push(await PtrSyncController.onFinished((result: PtrSyncResult) => {
        lastEventRef.current = Date.now();
        if (fadeTimerRef.current) clearTimeout(fadeTimerRef.current);

        if (result.success) {
          setPtrState({ phase: 'done', success: true });
          notifySuccess('PTR sync completed', 'PTR Sync');
          // Fade out success after 4s
          fadeTimerRef.current = setTimeout(() => {
            setPtrState({ phase: 'idle' });
          }, 4000);
        } else if (result.error === 'Cancelled') {
          setPtrState({ phase: 'idle' });
        } else {
          const error = result.error || 'Connection failed';
          setPtrState({ phase: 'done', success: false, error });
          notifyError(error, 'PTR Sync Failed');
          // Keep error visible longer (8s) so user can see it
          fadeTimerRef.current = setTimeout(() => {
            setPtrState({ phase: 'idle' });
          }, 8000);
        }
      }));
      push(await PtrSyncController.onPhaseChanged(() => {
        // no-op for now; progress row already carries phase. Keeping listener
        // here ensures frontend stays subscribed to the explicit phase channel.
      }));
      push(await PtrSyncController.onBootstrapStarted((payload) => {
        lastEventRef.current = Date.now();
        if (bootstrapFadeTimerRef.current) clearTimeout(bootstrapFadeTimerRef.current);
        const p = payload as { mode?: string; service_id?: number };
        setPtrBootstrapState({
          phase: 'running',
          status: {
            running: true,
            phase: 'probe',
            mode: p.mode ?? 'import',
            service_id: p.service_id,
          },
        });
      }));
      push(await PtrSyncController.onBootstrapProgress((payload) => {
        lastEventRef.current = Date.now();
        const p = payload as {
          phase?: string;
          stage?: string;
          service_id?: number;
          rows_done?: number;
          rows_total?: number;
          rows_done_stage?: number;
          rows_total_stage?: number;
          rows_per_sec?: number;
          eta_seconds?: number;
        };
        setPtrBootstrapState((prev) => {
          const base = prev.phase === 'running'
            ? prev.status
            : { running: true, phase: p.phase ?? 'running', mode: 'import' } as PtrBootstrapStatus;
          return {
            phase: 'running',
            status: {
              ...base,
              running: true,
              phase: p.phase ?? base.phase,
              stage: p.stage ?? base.stage,
              service_id: p.service_id ?? base.service_id,
              rows_done: p.rows_done ?? base.rows_done,
              rows_total: p.rows_total ?? base.rows_total,
              rows_done_stage: p.rows_done_stage ?? base.rows_done_stage,
              rows_total_stage: p.rows_total_stage ?? base.rows_total_stage,
              rows_per_sec: p.rows_per_sec ?? base.rows_per_sec,
              eta_seconds: p.eta_seconds ?? base.eta_seconds,
              updated_at: new Date().toISOString(),
            },
          };
        });
      }));
      push(await PtrSyncController.onBootstrapFinished(() => {
        lastEventRef.current = Date.now();
        if (bootstrapFadeTimerRef.current) clearTimeout(bootstrapFadeTimerRef.current);
        setPtrBootstrapState({ phase: 'done', success: true });
        bootstrapFadeTimerRef.current = setTimeout(() => {
          setPtrBootstrapState({ phase: 'idle' });
        }, 4000);
      }));
      push(await PtrSyncController.onBootstrapFailed((payload) => {
        lastEventRef.current = Date.now();
        if (bootstrapFadeTimerRef.current) clearTimeout(bootstrapFadeTimerRef.current);
        const p = payload as { error?: string };
        setPtrBootstrapState({ phase: 'done', success: false, error: p.error || 'Bootstrap failed' });
        bootstrapFadeTimerRef.current = setTimeout(() => {
          setPtrBootstrapState({ phase: 'idle' });
        }, 8000);
      }));
      // PBI-042: Use structured event payloads for subscription lifecycle.
      push(await SubscriptionController.onStarted((event: SubscriptionStartedEvent) => {
        lastEventRef.current = Date.now();
        const existingTimer = subFinishTimersRef.current.get(event.subscription_id);
        if (existingTimer) {
          clearTimeout(existingTimer);
          subFinishTimersRef.current.delete(event.subscription_id);
        }
        setSubs((prev) => {
          const next = new Map(prev);
          const resolvedName =
            (event.subscription_name ?? '').trim() ||
            subNameByIdRef.current.get(event.subscription_id) ||
            prev.get(event.subscription_id)?.subscription_name ||
            `Subscription ${event.subscription_id}`;
          next.set(event.subscription_id, {
            subscription_id: event.subscription_id,
            subscription_name: resolvedName,
            query_name: event.query_name,
            files_downloaded: 0,
            files_skipped: 0,
            pages_fetched: 0,
            status_text: 'Starting...',
          });
          return next;
        });
        void syncRunningSubscriptions(true).then((runningCount) => {
          if (runningCount > 0) SidebarController.requestRefresh();
        });
      }));
      push(await SubscriptionController.onProgress((p: SubscriptionProgressEvent) => {
        lastEventRef.current = Date.now();
        const existingTimer = subFinishTimersRef.current.get(p.subscription_id);
        if (existingTimer) {
          clearTimeout(existingTimer);
          subFinishTimersRef.current.delete(p.subscription_id);
        }
        setSubs((prev) => {
          const next = new Map(prev);
          const existing = next.get(p.subscription_id);
          const resolvedName =
            (p.subscription_name ?? '').trim() ||
            existing?.subscription_name ||
            subNameByIdRef.current.get(p.subscription_id) ||
            `Subscription ${p.subscription_id}`;
          next.set(p.subscription_id, {
            ...p,
            subscription_name: resolvedName,
            query_name: p.query_name ?? existing?.query_name,
          });
          return next;
        });
      }));
      push(await SubscriptionController.onFinished((event: SubscriptionFinishedEvent) => {
        lastEventRef.current = Date.now();
        const resolvedStatus =
          event.status === 'succeeded'
            ? 'Completed'
            : event.status === 'cancelled'
              ? 'Cancelled'
              : 'Failed';
        setSubs((prev) => {
          const next = new Map(prev);
          const existing = next.get(event.subscription_id);
          const resolvedName =
            (event.subscription_name ?? '').trim() ||
            existing?.subscription_name ||
            subNameByIdRef.current.get(event.subscription_id) ||
            `Subscription ${event.subscription_id}`;
          next.set(event.subscription_id, {
            subscription_id: event.subscription_id,
            subscription_name: resolvedName,
            query_name: event.query_name ?? existing?.query_name,
            files_downloaded: event.files_downloaded ?? existing?.files_downloaded ?? 0,
            files_skipped: event.files_skipped ?? existing?.files_skipped ?? 0,
            pages_fetched: existing?.pages_fetched ?? 0,
            status_text: resolvedStatus,
          });
          return next;
        });
        const existingTimer = subFinishTimersRef.current.get(event.subscription_id);
        if (existingTimer) clearTimeout(existingTimer);
        const removeTimer = setTimeout(() => {
          setSubs((prev) => {
            const next = new Map(prev);
            next.delete(event.subscription_id);
            return next;
          });
          subFinishTimersRef.current.delete(event.subscription_id);
        }, event.status === 'failed' ? 4500 : 2200);
        subFinishTimersRef.current.set(event.subscription_id, removeTimer);
        void syncRunningSubscriptions().then((runningCount) => {
          if (runningCount > 0) SidebarController.requestRefresh();
        });
      }));

      // PBI-039: Watchdog-only poll (10s). Skips when events are fresh (< 5s old).
      if (disposed) return;
      // Keep sidebar counters fresh even if progress events drop.
      subProgressPollTimer = setInterval(() => {
        void syncRunningSubscriptions().then((runningCount) => {
          if (runningCount > 0) SidebarController.requestRefresh();
        });
      }, 1000);
      pollTimer = setInterval(() => {
        const staleMs = Date.now() - lastEventRef.current;
        if (staleMs < 5000) return; // Events are fresh — skip poll.

        void syncRunningSubscriptions();
        Promise.all([
          PtrSyncController.isSyncing(),
          PtrSyncController.getSyncProgress(),
          PtrSyncController.getBootstrapStatus(),
        ]).then(([running, progress, bootstrap]) => {
          setPtrState((prev) => {
            if (running && prev.phase === 'idle') {
              return { phase: 'syncing', progress: progress ?? null };
            }
            if (running && prev.phase === 'syncing' && progress) {
              return { phase: 'syncing', progress };
            }
            if (!running && prev.phase === 'syncing') {
              return { phase: 'idle' };
            }
            return prev;
          });
          setPtrBootstrapState((prev) => {
            if (bootstrap.running) {
              return { phase: 'running', status: bootstrap };
            }
            if (!bootstrap.running && prev.phase === 'running') {
              return { phase: 'idle' };
            }
            return prev;
          });
        }).catch((error) => {
          logBestEffortError('sidebarJobStatus.watchdogPoll', error);
        });
      }, 10000);
    };
    setup();

    return () => {
      disposed = true;
      cleanups.forEach((fn) => fn());
      if (pollTimer) clearInterval(pollTimer);
      if (subProgressPollTimer) clearInterval(subProgressPollTimer);
      if (fadeTimerRef.current) clearTimeout(fadeTimerRef.current);
      if (bootstrapFadeTimerRef.current) clearTimeout(bootstrapFadeTimerRef.current);
      for (const timer of subFinishTimersRef.current.values()) clearTimeout(timer);
      subFinishTimersRef.current.clear();
    };
  }, [syncRunningSubscriptions]);

  const showPtr = ptrState.phase !== 'idle';
  const showBootstrap = ptrBootstrapState.phase !== 'idle';
  const hasJobs = showPtr || showBootstrap || subs.size > 0;
  if (!hasJobs) return null;

  // PTR progress bar — based on updates_processed / updates_total (hash count, like Hydrus)
  const ptrPct = ptrState.phase === 'syncing' && ptrState.progress
    && ptrState.progress.updates_total > 0 && ptrState.progress.updates_processed > 0
    ? (ptrState.progress.updates_processed / ptrState.progress.updates_total) * 100
    : 0;

  // Stay indeterminate until actual downloads start completing
  const ptrIndeterminate = ptrState.phase === 'syncing'
    && (!ptrState.progress || ptrState.progress.updates_processed === 0);

  // Phase label for row 1 (next to bold "PTR Sync")
  const ptrPhaseText = (() => {
    if (ptrState.phase === 'syncing') {
      const p = ptrState.progress;
      if (!p || !p.phase) return 'Waiting...';
      if (p.phase === 'metadata') return 'Fetching metadata...';
      if (p.phase === 'definitions') return 'Inserting definitions...';
      if (p.phase === 'schema_rebuild') return 'Rebuilding PTR schema...';
      const label = p.phase === 'downloading' ? 'Downloading' : 'Processing';
      if (p.updates_total > 0) {
        return `${label} · ${p.updates_processed.toLocaleString()} / ${p.updates_total.toLocaleString()}`;
      }
      return `${label}...`;
    }
    if (ptrState.phase === 'done' && ptrState.success) return 'Complete';
    if (ptrState.phase === 'done' && !ptrState.success) return ptrState.error;
    return '';
  })();

  const ptrIsError = ptrState.phase === 'done' && !ptrState.success;

  return (
    <div className={st.root}>
      {showPtr && (
        <div className={`${st.jobCard} ${ptrIsError ? st.jobCardError : ''}`}>
          <div className={ptrIsError ? st.jobIconError : st.jobIcon}>
            {ptrIsError ? (
              <IconAlertTriangle size={14} />
            ) : ptrState.phase === 'done' && ptrState.success ? (
              <IconCheck size={14} />
            ) : (
              <IconCloud size={14} />
            )}
          </div>
          <div className={st.jobNameRow}>
            <span className={st.jobName}>PTR Sync</span>
            {ptrPhaseText && (
              <span className={ptrIsError ? st.jobPhaseError : st.jobPhase}>
                {ptrPhaseText}
              </span>
            )}
          </div>
          {ptrState.phase === 'syncing' && (
            <div className={st.jobProgressRow}>
              <div className={st.jobProgress}>
                {ptrIndeterminate ? (
                  <div className={st.progressIndeterminate} />
                ) : (
                  <div className={st.progressFill} style={{ width: `${ptrPct}%` }} />
                )}
              </div>
            </div>
          )}
        </div>
      )}

      {showBootstrap && (
        <div className={st.jobCard}>
          <div className={st.jobIcon}>
            <IconCloud size={14} />
          </div>
          <div className={st.jobNameRow}>
            <span className={st.jobName}>PTR Bootstrap</span>
            <span className={st.jobPhase}>
              {ptrBootstrapState.phase === 'running'
                ? `${ptrBootstrapState.status.stage || ptrBootstrapState.status.phase || 'running'}${
                    ptrBootstrapState.status.rows_total_stage && typeof ptrBootstrapState.status.rows_done_stage === 'number'
                      ? ` · ${ptrBootstrapState.status.rows_done_stage.toLocaleString()} / ${ptrBootstrapState.status.rows_total_stage.toLocaleString()}`
                      : ''
                  }${
                    typeof ptrBootstrapState.status.rows_per_sec === 'number' && ptrBootstrapState.status.rows_per_sec > 0
                      ? ` · ${Math.round(ptrBootstrapState.status.rows_per_sec).toLocaleString()} rows/s`
                      : ''
                  }${
                    typeof ptrBootstrapState.status.eta_seconds === 'number' && ptrBootstrapState.status.eta_seconds > 0
                      ? ` · ETA ${Math.ceil(ptrBootstrapState.status.eta_seconds)}s`
                      : ''
                  }`
                : ptrBootstrapState.phase === 'done' && ptrBootstrapState.success
                  ? 'Complete'
                  : ptrBootstrapState.phase === 'done' && !ptrBootstrapState.success
                    ? ptrBootstrapState.error
                    : ''}
            </span>
          </div>
          {ptrBootstrapState.phase === 'running' && (
            <div className={st.jobProgressRow}>
              <div className={st.jobProgress}>
                {ptrBootstrapState.status.rows_total_stage && ptrBootstrapState.status.rows_total_stage > 0
                  ? <div
                      className={st.progressBar}
                      style={{
                        width: `${Math.min(100, ((ptrBootstrapState.status.rows_done_stage ?? 0) / ptrBootstrapState.status.rows_total_stage) * 100)}%`,
                      }}
                    />
                  : <div className={st.progressIndeterminate} />
                }
              </div>
            </div>
          )}
        </div>
      )}

      {/* PBI-044: Scroll container capped at 3 subscription cards */}
      {subs.size > 0 && (
        <div className={st.subList}>
          {[...subs.values()].map((sub) => (
            <div key={sub.subscription_id} className={st.jobCard}>
              <div className={st.jobIcon}>
                <IconDownload size={14} />
              </div>
              {/* PBI-045: Two-row layout — top row has name + right-aligned status */}
              <div className={st.jobNameRow}>
                <span className={st.jobName}>
                  {(sub.query_name ?? '').trim() || sub.subscription_name}
                </span>
                <span className={st.jobPhase}>
                  {sub.status_text || 'fetching'}
                </span>
              </div>
              <div className={st.jobProgressRow}>
                <div className={st.jobProgress}>
                  {sub.files_downloaded > 0
                    ? <div className={st.progressFill} style={{ width: '100%' }} />
                    : <div className={st.progressIndeterminate} />
                  }
                </div>
                <span className={st.jobCount}>
                  {sub.files_downloaded.toLocaleString()} downloaded
                  {' · '}
                  {sub.files_skipped.toLocaleString()} skipped
                  {' · '}
                  {sub.pages_fetched.toLocaleString()} pages
                </span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
