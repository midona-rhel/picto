import { useEffect, useMemo, useRef, useState } from 'react';
import { IconAlertTriangle, IconCheck, IconCloud, IconDownload } from '@tabler/icons-react';

import type { PtrBootstrapStatus, PtrSyncProgress } from '../../controllers/ptrSyncController';
import { useTaskRuntimeStore } from '../../stores/taskRuntimeStore';
import st from './SidebarJobStatus.module.css';

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
  const ptrSyncing = useTaskRuntimeStore((s) => s.ptrSyncing);
  const ptrProgress = useTaskRuntimeStore((s) => s.ptrProgress);
  const ptrLastResult = useTaskRuntimeStore((s) => s.ptrLastResult);
  const ptrBootstrapStatus = useTaskRuntimeStore((s) => s.ptrBootstrapStatus);
  const subscriptionProgressById = useTaskRuntimeStore((s) => s.subscriptionProgressById);

  const [ptrState, setPtrState] = useState<PtrState>({ phase: 'idle' });
  const [ptrBootstrapState, setPtrBootstrapState] = useState<PtrBootstrapState>({ phase: 'idle' });
  const ptrFadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const bootstrapFadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastPtrResultRef = useRef<string | null>(null);
  const wasBootstrapRunningRef = useRef(false);

  const subs = useMemo(() => {
    return [...subscriptionProgressById.values()].sort((a, b) => a.subscription_id.localeCompare(b.subscription_id));
  }, [subscriptionProgressById]);

  useEffect(() => {
    if (ptrFadeTimerRef.current) {
      clearTimeout(ptrFadeTimerRef.current);
      ptrFadeTimerRef.current = null;
    }

    if (ptrSyncing) {
      setPtrState((prev) => {
        if (prev.phase === 'syncing' && prev.progress === (ptrProgress ?? null)) return prev;
        return { phase: 'syncing', progress: ptrProgress ?? null };
      });
      return;
    }

    if (!ptrLastResult) {
      setPtrState((prev) => (prev.phase === 'idle' ? prev : { phase: 'idle' }));
      return;
    }

    const resultKey = `${ptrLastResult.success ? 'ok' : 'err'}:${ptrLastResult.error ?? ''}:${ptrLastResult.updates_processed ?? -1}`;
    if (lastPtrResultRef.current === resultKey) return;
    lastPtrResultRef.current = resultKey;

    if (ptrLastResult.success) {
      setPtrState((prev) => {
        if (prev.phase === 'done' && prev.success === true) return prev;
        return { phase: 'done', success: true };
      });
      ptrFadeTimerRef.current = setTimeout(() => {
        setPtrState((prev) => (prev.phase === 'idle' ? prev : { phase: 'idle' }));
      }, 4000);
      return;
    }

    if (ptrLastResult.error === 'Cancelled') {
      setPtrState((prev) => (prev.phase === 'idle' ? prev : { phase: 'idle' }));
      return;
    }

    setPtrState((prev) => {
      const nextError = ptrLastResult.error || 'Connection failed';
      if (prev.phase === 'done' && prev.success === false && prev.error === nextError) return prev;
      return { phase: 'done', success: false, error: nextError };
    });
    ptrFadeTimerRef.current = setTimeout(() => {
      setPtrState((prev) => (prev.phase === 'idle' ? prev : { phase: 'idle' }));
    }, 8000);
  }, [ptrSyncing, ptrProgress, ptrLastResult]);

  useEffect(() => {
    if (bootstrapFadeTimerRef.current) {
      clearTimeout(bootstrapFadeTimerRef.current);
      bootstrapFadeTimerRef.current = null;
    }

    const status = ptrBootstrapStatus;
    if (status?.running) {
      wasBootstrapRunningRef.current = true;
      setPtrBootstrapState((prev) => {
        if (prev.phase === 'running' && prev.status === status) return prev;
        return { phase: 'running', status };
      });
      return;
    }

    if (wasBootstrapRunningRef.current) {
      wasBootstrapRunningRef.current = false;
      if (status?.last_error) {
        const bootstrapError = status.last_error;
        setPtrBootstrapState((prev) => {
          if (prev.phase === 'done' && prev.success === false && prev.error === bootstrapError) return prev;
          return { phase: 'done', success: false, error: bootstrapError };
        });
        bootstrapFadeTimerRef.current = setTimeout(() => {
          setPtrBootstrapState((prev) => (prev.phase === 'idle' ? prev : { phase: 'idle' }));
        }, 8000);
      } else {
        setPtrBootstrapState((prev) => {
          if (prev.phase === 'done' && prev.success === true) return prev;
          return { phase: 'done', success: true };
        });
        bootstrapFadeTimerRef.current = setTimeout(() => {
          setPtrBootstrapState((prev) => (prev.phase === 'idle' ? prev : { phase: 'idle' }));
        }, 4000);
      }
      return;
    }

    setPtrBootstrapState((prev) => (prev.phase === 'idle' ? prev : { phase: 'idle' }));
  }, [ptrBootstrapStatus]);

  useEffect(() => {
    return () => {
      if (ptrFadeTimerRef.current) clearTimeout(ptrFadeTimerRef.current);
      if (bootstrapFadeTimerRef.current) clearTimeout(bootstrapFadeTimerRef.current);
    };
  }, []);

  const showPtr = ptrState.phase !== 'idle';
  const showBootstrap = ptrBootstrapState.phase !== 'idle';
  const hasJobs = showPtr || showBootstrap || subs.length > 0;
  if (!hasJobs) return null;

  const ptrPct = ptrState.phase === 'syncing' && ptrState.progress
    && ptrState.progress.updates_total > 0 && ptrState.progress.updates_processed > 0
    ? (ptrState.progress.updates_processed / ptrState.progress.updates_total) * 100
    : 0;

  const ptrIndeterminate = ptrState.phase === 'syncing'
    && (!ptrState.progress || ptrState.progress.updates_processed === 0);

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

      {subs.length > 0 && (
        <div className={st.subList}>
          {subs.map((sub) => (
            <div key={sub.subscription_id} className={st.jobCard}>
              <div className={st.jobIcon}>
                <IconDownload size={14} />
              </div>
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
