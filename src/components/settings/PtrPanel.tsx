import { useState, useEffect, useRef } from 'react';
import {
  Text,
  Switch,
  Select,
  TextInput,
  Progress,
  Loader,
} from '@mantine/core';
import { api, open as openDialog } from '#desktop/api';
import { TextButton } from '../ui/TextButton';
import { SettingsBlock, SettingsRow, SettingsInputGroup } from './ui';
import {
  PtrSyncController,
  type PtrBootstrapStatus,
  type PtrSyncProgress,
  type PtrSyncResult,
} from '../../controllers/ptrSyncController';
import { logBestEffortError, runBestEffort, runCriticalAction } from '../../lib/asyncOps';

interface AppSettings {
  ptrServerUrl: string | null;
  ptrAccessKey: string | null;
  ptrEnabled: boolean;
  ptrAutoSync: boolean;
  ptrSyncSchedule: string;
  ptrLastSyncTime: string | null;
  ptrDataPath: string | null;
  [key: string]: unknown;
}

interface PtrStats {
  tag_count: number;
  file_stub_count: number;
  mapping_count: number;
  sibling_count: number;
  parent_count: number;
  sync_position: number;
}

function formatNumber(n: number | null | undefined): string {
  if (typeof n !== 'number' || !Number.isFinite(n)) return '0';
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    return `${m}m ${s}s`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

function formatRelativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (isNaN(then)) return 'unknown';
  const diffMs = Date.now() - then;
  const diffSec = Math.floor(diffMs / 1000);
  if (diffSec < 60) return 'just now';
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDays = Math.floor(diffHr / 24);
  if (diffDays === 1) return 'yesterday';
  return `${diffDays} days ago`;
}

function formatMs(v: unknown): string {
  const n = typeof v === 'number' ? v : Number(v);
  if (!Number.isFinite(n)) return '-';
  return `${n.toFixed(1)}ms`;
}

function formatInt(v: unknown): string {
  const n = typeof v === 'number' ? v : Number(v);
  if (!Number.isFinite(n)) return '-';
  return Math.round(n).toLocaleString();
}

const DEFAULT_PTR_SERVER = 'https://ptr.hydrus.network:45871';
const DEFAULT_PTR_ACCESS_KEY = '4a285629721ca442541ef2c15ea17d1f7f7578b0c3f4f5f2a05f8f0ab297786f';

export function PtrPanel() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [stats, setStats] = useState<PtrStats | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncProgress, setSyncProgress] = useState<PtrSyncProgress | null>(null);
  const [syncError, setSyncError] = useState<string | null>(null);
  const [syncDone, setSyncDone] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [bootstrapDir, setBootstrapDir] = useState('');
  const [bootstrapStatus, setBootstrapStatus] = useState<PtrBootstrapStatus | null>(null);
  const [bootstrapError, setBootstrapError] = useState<string | null>(null);
  const [perfBreakdown, setPerfBreakdown] = useState<Record<string, unknown> | null>(null);
  const syncStartRef = useRef(0);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const doneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    loadSettings();
    loadStats();
    runBestEffort('ptr.getBootstrapStatus.initial', PtrSyncController.getBootstrapStatus().then(setBootstrapStatus));

    PtrSyncController.isSyncing().then((running) => {
      if (running) {
        setSyncing(true);
        startTimer();
      }
    }).catch((error) => {
      logBestEffortError('ptr.isSyncing.initial', error);
    });

    const cleanups: Array<() => void> = [];
    let disposed = false;
    const push = (fn: () => void) => { if (disposed) fn(); else cleanups.push(fn); };

    const setup = async () => {
      push(await PtrSyncController.onStarted(() => {
        setSyncing(true);
        setSyncProgress(null);
        setSyncError(null);
        setCancelling(false);
        startTimer();
      }));
      push(await PtrSyncController.onProgress((progress) => {
        setSyncProgress(progress);
      }));
      push(await PtrSyncController.onFinished((result: PtrSyncResult) => {
        setSyncing(false);
        setSyncProgress(null);
        setCancelling(false);
        stopTimer();
        if (result.success) {
          setSyncError(null);
          setSyncDone(true);
          if (doneTimerRef.current) clearTimeout(doneTimerRef.current);
          doneTimerRef.current = setTimeout(() => setSyncDone(false), 4000);
        } else if (result.error === 'Cancelled') {
          setSyncError(null);
        } else {
          setSyncError(result.error || 'Unknown error');
        }
        loadStats();
        loadSettings();
        runBestEffort(
          'ptr.getSyncPerfBreakdown.onFinished',
          PtrSyncController.getSyncPerfBreakdown().then((perf) => setPerfBreakdown(perf as Record<string, unknown>)),
        );
      }));
      push(await PtrSyncController.onBootstrapStarted(() => {
        runBestEffort('ptr.getBootstrapStatus.onBootstrapStarted', PtrSyncController.getBootstrapStatus().then(setBootstrapStatus));
      }));
      push(await PtrSyncController.onBootstrapProgress(() => {
        runBestEffort('ptr.getBootstrapStatus.onBootstrapProgress', PtrSyncController.getBootstrapStatus().then(setBootstrapStatus));
      }));
      push(await PtrSyncController.onBootstrapFinished(() => {
        runBestEffort('ptr.getBootstrapStatus.onBootstrapFinished', PtrSyncController.getBootstrapStatus().then(setBootstrapStatus));
      }));
      push(await PtrSyncController.onBootstrapFailed((payload) => {
        const p = payload as { error?: string };
        setBootstrapError(p.error || 'Bootstrap failed');
        runBestEffort('ptr.getBootstrapStatus.onBootstrapFailed', PtrSyncController.getBootstrapStatus().then(setBootstrapStatus));
      }));
    };
    setup();

    return () => {
      disposed = true;
      cleanups.forEach((fn) => fn());
      stopTimer();
      if (doneTimerRef.current) clearTimeout(doneTimerRef.current);
    };
  }, []);

  // Fallback poll: fetch progress + detect sync end even if events are lost
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const [running, progress, bootstrap, compact] = await Promise.all([
          PtrSyncController.isSyncing(),
          PtrSyncController.getSyncProgress(),
          PtrSyncController.getBootstrapStatus(),
          PtrSyncController.getCompactIndexStatus().catch((error) => {
            logBestEffortError('ptr.getCompactIndexStatus.poll', error);
            return null;
          }),
        ]);
        const mergedBootstrap = compact
          ? {
              ...bootstrap,
              stage: compact.running ? 'compact_build' : bootstrap.stage,
              rows_done_stage: compact.rows_done_stage ?? bootstrap.rows_done_stage,
              rows_total_stage: compact.rows_total_stage ?? bootstrap.rows_total_stage,
              rows_per_sec: compact.rows_per_sec ?? bootstrap.rows_per_sec,
              checkpoint: compact.checkpoint ?? bootstrap.checkpoint,
            }
          : bootstrap;
        setBootstrapStatus(mergedBootstrap);
        // Only fetch expensive diagnostics when sync or bootstrap is active.
        if (running || bootstrap.running) {
          const perf = await PtrSyncController.getSyncPerfBreakdown();
          setPerfBreakdown(perf as Record<string, unknown>);
        }
        if (!running) {
          if (syncing || cancelling) {
            setSyncing(false);
            setCancelling(false);
            setSyncProgress(null);
            stopTimer();
            loadStats();
            loadSettings();
          }
        } else if (progress) {
          setSyncProgress(progress);
        }
      } catch (error) {
        logBestEffortError('ptr.poll', error);
      }
    }, 1500);
    return () => clearInterval(interval);
  }, [syncing, cancelling]);

  const stopTimer = () => {
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
  };

  const startTimer = () => {
    stopTimer();
    syncStartRef.current = Date.now();
    setElapsed(0);
    timerRef.current = setInterval(() => {
      setElapsed((Date.now() - syncStartRef.current) / 1000);
    }, 1000);
  };

  const loadSettings = async () => {
    try {
      const result = await api.settings.get() as AppSettings;
      setSettings(result);
    } catch (err) {
      console.error('Failed to load settings:', err);
    }
  };

  const loadStats = async () => {
    try {
      const result = await api.ptr.getStatus() as PtrStats;
      setStats(result);
    } catch (err) {
      console.error('Failed to load PTR stats:', err);
    }
  };

  const saveSetting = async (patch: Partial<AppSettings>) => {
    if (!settings) return;
    const updated = { ...settings, ...patch };
    setSettings(updated);
    try {
      await api.settings.save(updated);
    } catch (err) {
      console.error('Failed to save settings:', err);
    }
  };

  const handleSync = async () => {
    try {
      setSyncError(null);
      setSyncing(true);
      setSyncProgress(null);
      startTimer();
      await PtrSyncController.sync();
    } catch (err) {
      setSyncError(String(err));
      stopTimer();
      setSyncing(false);
      setSyncProgress(null);
    }
  };

  const handleChooseDataPath = async () => {
    try {
      const selected = await openDialog({ properties: ['openDirectory'] });
      if (selected && typeof selected === 'string') {
        saveSetting({ ptrDataPath: selected });
      }
    } catch {
      // intentional-noop-catch: dialog may be unavailable, manual path input still works.
    }
  };

  const handleChooseBootstrapDir = async () => {
    try {
      const selected = await openDialog({ properties: ['openDirectory'] });
      if (selected && typeof selected === 'string') {
        setBootstrapDir(selected);
      }
    } catch {
      // intentional-noop-catch: dialog cancellation is expected.
    }
  };

  const runBootstrapDryRun = async () => {
    try {
      setBootstrapError(null);
      if (!bootstrapDir.trim()) {
        setBootstrapError('Please select a Hydrus snapshot directory.');
        return;
      }
      await PtrSyncController.bootstrapFromHydrusSnapshot({
        snapshot_dir: bootstrapDir,
        mode: 'dry_run',
      });
      const status = await PtrSyncController.getBootstrapStatus();
      setBootstrapStatus(status);
    } catch (err) {
      setBootstrapError(String(err));
    }
  };

  const runBootstrapImport = async () => {
    try {
      setBootstrapError(null);
      if (!bootstrapDir.trim()) {
        setBootstrapError('Please select a Hydrus snapshot directory.');
        return;
      }
      await PtrSyncController.bootstrapFromHydrusSnapshot({
        snapshot_dir: bootstrapDir,
        mode: 'import',
      });
      const status = await PtrSyncController.getBootstrapStatus();
      setBootstrapStatus(status);
    } catch (err) {
      setBootstrapError(String(err));
    }
  };

  if (!settings) return null;

  const ptrEnabled = settings.ptrEnabled;
  const dimmed = !ptrEnabled;

  return (
    <>
      {/* Connection */}
      <SettingsBlock title="Connection" description="Community tag database with millions of file-tag mappings.">
        <SettingsRow label="Enable PTR">
          <Switch size="xs" checked={ptrEnabled} onChange={(e) => saveSetting({ ptrEnabled: e.currentTarget.checked })} />
        </SettingsRow>
      </SettingsBlock>

      {/* Server */}
      <SettingsBlock title="Server" dimmed={dimmed}>
        <SettingsRow label="Server URL">
          <TextInput
            size="xs" w={260}
            value={settings.ptrServerUrl ?? DEFAULT_PTR_SERVER}
            onChange={(e) => saveSetting({ ptrServerUrl: e.currentTarget.value || null })}
          />
        </SettingsRow>
        <SettingsRow label="Access Key" separator>
          <TextInput
            size="xs" w={260}
            value={settings.ptrAccessKey ?? DEFAULT_PTR_ACCESS_KEY}
            readOnly={!settings.ptrAccessKey}
            onChange={(e) => saveSetting({ ptrAccessKey: e.currentTarget.value || null })}
            styles={{ input: { fontFamily: 'monospace', fontSize: 'var(--font-size-2xs)' } }}
          />
        </SettingsRow>
      </SettingsBlock>

      {/* Database Location */}
      <SettingsBlock title="Database Location" dimmed={dimmed} description="The PTR database can be very large (50+ GB). Choose where to store it.">
        <SettingsInputGroup>
          <TextInput
            size="xs" placeholder="Default (alongside library)"
            value={settings.ptrDataPath ?? ''}
            onChange={(e) => saveSetting({ ptrDataPath: e.currentTarget.value || null })}
            style={{ flex: 1 }}
          />
          <TextButton compact onClick={handleChooseDataPath}>Browse</TextButton>
        </SettingsInputGroup>
      </SettingsBlock>

      {/* Auto-sync */}
      <SettingsBlock title="Auto-sync" dimmed={dimmed}>
        <SettingsRow label="Auto-sync">
          <Switch size="xs" checked={settings.ptrAutoSync} onChange={(e) => saveSetting({ ptrAutoSync: e.currentTarget.checked })} />
        </SettingsRow>
        {settings.ptrAutoSync && (
          <SettingsRow label="Schedule" separator>
            <Select
              size="xs" w={120} value={settings.ptrSyncSchedule}
              onChange={(v) => v && saveSetting({ ptrSyncSchedule: v })}
              data={[
                { value: 'daily', label: 'Daily' },
                { value: 'weekly', label: 'Weekly' },
                { value: 'monthly', label: 'Monthly' },
              ]}
            />
          </SettingsRow>
        )}
        {settings.ptrLastSyncTime && (
          <SettingsRow label="Last synced" separator>
            <Text size="xs" c="dimmed">{formatRelativeTime(settings.ptrLastSyncTime)}</Text>
          </SettingsRow>
        )}
      </SettingsBlock>

      {/* Database Stats */}
      {stats && (
        <SettingsBlock title="Database Stats" dimmed={dimmed}>
          <SettingsRow label="Files">
            <Text size="xs" c="dimmed" ff="monospace">{formatNumber(stats.file_stub_count)}</Text>
          </SettingsRow>
          <SettingsRow label="Tags" separator>
            <Text size="xs" c="dimmed" ff="monospace">{formatNumber(stats.tag_count)}</Text>
          </SettingsRow>
          <SettingsRow label="Mappings" separator>
            <Text size="xs" c="dimmed" ff="monospace">{formatNumber(stats.mapping_count)}</Text>
          </SettingsRow>
          <SettingsRow label="Siblings" separator>
            <Text size="xs" c="dimmed" ff="monospace">{formatNumber(stats.sibling_count)}</Text>
          </SettingsRow>
          <SettingsRow label="Parents" separator>
            <Text size="xs" c="dimmed" ff="monospace">{formatNumber(stats.parent_count)}</Text>
          </SettingsRow>
          {stats.sync_position >= 0 ? (
            <SettingsRow label="Sync position" separator>
              <Text size="xs" c="dimmed" ff="monospace">#{stats.sync_position.toLocaleString()}</Text>
            </SettingsRow>
          ) : (
            <Text size="xs" c="dimmed" mt={8}>
              Not yet synced. Click "Sync Now" to download community tags.
            </Text>
          )}
        </SettingsBlock>
      )}

      {/* Errors */}
      {syncError && (
        <SettingsBlock borderColor="var(--color-red, #e03131)">
          <SettingsRow label="Sync Failed">
            <TextButton compact onClick={() => setSyncError(null)}>Dismiss</TextButton>
          </SettingsRow>
          <Text size="xs" c="dimmed" mt={4}>{syncError}</Text>
        </SettingsBlock>
      )}
      {bootstrapError && (
        <SettingsBlock borderColor="var(--color-red, #e03131)">
          <SettingsRow label="Bootstrap Failed">
            <TextButton compact onClick={() => setBootstrapError(null)}>Dismiss</TextButton>
          </SettingsRow>
          <Text size="xs" c="dimmed" mt={4}>{bootstrapError}</Text>
        </SettingsBlock>
      )}

      {/* Bootstrap */}
      <SettingsBlock title="Hydrus Snapshot Bootstrap" dimmed={dimmed} description="Bootstrap PTR from local Hydrus snapshot. Import auto-finalizes cursor, starts delta sync, and continues compact index build in background.">
        <SettingsInputGroup mb={8}>
          <TextInput
            size="xs" value={bootstrapDir}
            onChange={(e) => setBootstrapDir(e.currentTarget.value)}
            style={{ flex: 1 }}
          />
          <TextButton compact onClick={handleChooseBootstrapDir}>Browse</TextButton>
        </SettingsInputGroup>
        <SettingsInputGroup>
          <TextButton compact onClick={runBootstrapDryRun} disabled={!!bootstrapStatus?.running}>Dry Run</TextButton>
          <TextButton compact onClick={runBootstrapImport} disabled={!!bootstrapStatus?.running}>Import</TextButton>
          {bootstrapStatus?.running && (
            <TextButton
              compact
              danger
              onClick={() => {
                runCriticalAction('Cancel Failed', 'ptr.cancelBootstrap', PtrSyncController.cancelBootstrap());
              }}
            >
              Cancel
            </TextButton>
          )}
        </SettingsInputGroup>
        {bootstrapStatus && (
          <Text size="xs" c="dimmed" mt={8}>
            Status: {bootstrapStatus.running ? 'running' : 'idle'}
            {bootstrapStatus.stage ? ` · stage=${bootstrapStatus.stage}` : ''}
            {bootstrapStatus.phase ? ` · phase=${bootstrapStatus.phase}` : ''}
            {bootstrapStatus.service_id ? ` · service=${bootstrapStatus.service_id}` : ''}
            {typeof bootstrapStatus.rows_done_stage === 'number' && typeof bootstrapStatus.rows_total_stage === 'number'
              ? ` · ${bootstrapStatus.rows_done_stage.toLocaleString()} / ${bootstrapStatus.rows_total_stage.toLocaleString()}`
              : typeof bootstrapStatus.rows_done === 'number' && typeof bootstrapStatus.rows_total === 'number'
              ? ` · ${bootstrapStatus.rows_done.toLocaleString()} / ${bootstrapStatus.rows_total.toLocaleString()}`
              : ''}
            {typeof bootstrapStatus.rows_per_sec === 'number' && bootstrapStatus.rows_per_sec > 0
              ? ` · ${Math.round(bootstrapStatus.rows_per_sec).toLocaleString()} rows/s`
              : ''}
            {typeof bootstrapStatus.eta_seconds === 'number' && bootstrapStatus.eta_seconds > 0
              ? ` · ETA ${formatDuration(bootstrapStatus.eta_seconds)}`
              : ''}
          </Text>
        )}
        {bootstrapStatus?.dry_run_result && (
          <Text size="xs" c="dimmed" mt={4}>
            Dry-run: mappings={formatNumber(bootstrapStatus.dry_run_result.counts.mappings)} · tags={formatNumber(bootstrapStatus.dry_run_result.counts.tag_defs)} · hashes={formatNumber(bootstrapStatus.dry_run_result.counts.hash_defs)} · max_index={formatNumber(bootstrapStatus.dry_run_result.counts.max_update_index)}
          </Text>
        )}
      </SettingsBlock>

      {/* Sync */}
      <SettingsBlock title="Sync" dimmed={dimmed}>
        {syncing ? (
          <>
            <SettingsInputGroup mb={8}>
              <Loader size={14} />
              <Text size="xs" c="dimmed" style={{ flex: 1 }}>
                {cancelling ? 'Cancelling...' : syncProgress ? 'Syncing...' : 'Connecting...'}
              </Text>
              <TextButton
                compact
                danger
                disabled={cancelling}
                onClick={() => {
                  setCancelling(true);
                  setSyncProgress(null);
                  runCriticalAction('Cancel Failed', 'ptr.cancelSync', PtrSyncController.cancelSync());
                }}
              >
                {cancelling ? 'Cancelling...' : 'Cancel'}
              </TextButton>
            </SettingsInputGroup>
            {syncProgress && syncProgress.latest_server_index > syncProgress.starting_index ? (() => {
              const range = syncProgress.latest_server_index - syncProgress.starting_index;
              const done = syncProgress.current_update_index - syncProgress.starting_index;
              const pct = (done / range) * 100;
              const eta = done > 0 ? (elapsed / done) * (range - done) : null;
              return (
                <>
                  <Progress value={pct} size="sm" animated />
                  <Text size="xs" c="dimmed" mt={4}>
                    Update #{syncProgress.current_update_index.toLocaleString()} / #{syncProgress.latest_server_index.toLocaleString()}
                    {' \u2014 '}{formatNumber(syncProgress.tags_added)} tags, {formatNumber(syncProgress.siblings_added)} siblings, {formatNumber(syncProgress.parents_added)} parents
                  </Text>
                  <Text size="xs" c="dimmed">
                    Elapsed: {formatDuration(elapsed)}{eta !== null && ` \u2014 ETA: ${formatDuration(eta)}`}
                  </Text>
                </>
              );
            })() : (
              <Text size="xs" c="dimmed">Connecting to PTR server... ({formatDuration(elapsed)})</Text>
            )}
          </>
        ) : syncDone ? (
          <TextButton compact onClick={() => setSyncDone(false)}>Sync Complete</TextButton>
        ) : (
          <TextButton compact onClick={handleSync}>Sync Now</TextButton>
        )}
      </SettingsBlock>

      {/* Perf */}
      {perfBreakdown && (
        <SettingsBlock title="Latest Sync Perf" dimmed={dimmed}>
          <Text size="xs" c="dimmed">
            Run: {formatInt((perfBreakdown.latest_run as Record<string, unknown> | undefined)?.updates_processed)} updates, {formatInt((perfBreakdown.latest_run as Record<string, unknown> | undefined)?.tags_added)} tags, elapsed {formatMs((perfBreakdown.latest_run as Record<string, unknown> | undefined)?.elapsed_ms)}
          </Text>
          <Text size="xs" c="dimmed" mt={2}>
            Chunk: defs={formatMs((perfBreakdown.latest_chunk as Record<string, unknown> | undefined)?.defs_insert_ms)} · resolve={formatMs((perfBreakdown.latest_chunk as Record<string, unknown> | undefined)?.resolve_ids_ms)} · write={formatMs((perfBreakdown.latest_chunk as Record<string, unknown> | undefined)?.content_write_ms)}
          </Text>
          <Text size="xs" c="dimmed" mt={2}>
            Apply: add={formatMs((perfBreakdown.latest_chunk as Record<string, unknown> | undefined)?.mapping_add_apply_ms)} · del={formatMs((perfBreakdown.latest_chunk as Record<string, unknown> | undefined)?.mapping_del_apply_ms)} · batches={formatInt((perfBreakdown.latest_chunk as Record<string, unknown> | undefined)?.total_batches)}
          </Text>
        </SettingsBlock>
      )}

    </>
  );
}
