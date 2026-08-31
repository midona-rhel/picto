import type { CloudSyncStatus } from '../../shared/types/generated/application/CloudSyncStatus';
import { t } from '../../i18n';

export interface SyncRateSample {
  at: number;
  remaining: number;
}

export interface CloudSyncPresentation {
  active: boolean;
  label: string;
  tone: 'positive' | 'neutral' | 'warning' | 'negative';
  completed: number;
  total: number | null;
  remaining: number | null;
  workKey: string | null;
}

export function presentCloudSync(status: CloudSyncStatus | null): CloudSyncPresentation {
  if (!status) {
    return {
      active: false,
      label: t("Unavailable"),
      tone: 'neutral',
      completed: 0,
      total: null,
      remaining: null,
      workKey: null,
    };
  }

  const pending = Math.max(0, status.pending_mutations) + Math.max(0, status.pending_blobs);
  const hasExactProgress = status.total_units !== null && status.total_units > 0;
  const active = status.state === 'reconciling';
  const completed = hasExactProgress ? Math.max(0, status.completed_units) : 0;
  const total = hasExactProgress ? Math.max(completed, status.total_units ?? 0) : null;
  const remaining = total !== null ? Math.max(0, total - completed) : active ? pending : null;
  const workKey = total !== null ? `phase:${status.phase}:${total}` : active ? 'pending' : null;

  if (status.state === 'error') {
    return { active: false, label: t("Needs attention"), tone: 'negative', completed, total, remaining: null, workKey: null };
  }
  if (status.state === 'offline') {
    return { active: false, label: t("Offline"), tone: 'warning', completed, total, remaining: null, workKey: null };
  }
  if (status.state === 'paused') {
    return { active: false, label: t("Paused"), tone: 'neutral', completed, total, remaining: null, workKey: null };
  }
  if (active) {
    return { active: true, label: t("Syncing"), tone: 'positive', completed, total, remaining, workKey };
  }
  return {
    active: false,
    label: t("Idle"),
    tone: 'neutral',
    completed,
    total,
    remaining: null,
    workKey: null,
  };
}

export function estimateRemainingSeconds(samples: SyncRateSample[], remaining: number): number | null {
  if (remaining <= 0 || samples.length < 2) return null;
  const latest = samples[samples.length - 1];
  const baseline = samples.find((sample) => sample.remaining > latest.remaining);
  if (!baseline) return null;
  const elapsedSeconds = (latest.at - baseline.at) / 1000;
  const completed = baseline.remaining - latest.remaining;
  if (elapsedSeconds < 1 || completed <= 0) return null;
  return Math.max(1, Math.ceil(remaining / (completed / elapsedSeconds)));
}

export function formatRemainingTime(seconds: number | null): string | null {
  if (seconds === null) return null;
  if (seconds < 60) return t('Less than a minute remaining');
  if (seconds < 3600) return t('About {value0} min remaining', { value0: Math.ceil(seconds / 60) });
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.ceil((seconds % 3600) / 60);
  return minutes > 0
    ? t('About {value0} hr {value1} min remaining', { value0: hours, value1: minutes })
    : t('About {value0} hr remaining', { value0: hours });
}

export function formatLastSync(value: string | null, now = Date.now()): string {
  if (!value) return t('Never');
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return value;
  const elapsedMinutes = Math.max(0, Math.floor((now - timestamp) / 60_000));
  if (elapsedMinutes < 1) return t('Just now');
  if (elapsedMinutes < 60) return t('{value0} min ago', { value0: elapsedMinutes });
  const elapsedHours = Math.floor(elapsedMinutes / 60);
  if (elapsedHours < 24) return t('{value0} hr ago', { value0: elapsedHours });
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(timestamp);
}
