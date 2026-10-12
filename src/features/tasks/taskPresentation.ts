import { t } from '../../i18n';
import type { ForegroundTask } from '../../shared/types/generated/application/ForegroundTask';

export type TaskPhase = 'finding' | 'importing' | 'paused' | 'removing' | 'finished' | 'stopped' | 'undone';

export function taskPhase(task: ForegroundTask): TaskPhase {
  switch (task.state) {
    case 'paused': return 'paused';
    case 'reverting': return 'removing';
    case 'finished': return 'finished';
    case 'stopped': return 'stopped';
    case 'reverted': return 'undone';
    default: return task.scan_complete ? 'importing' : 'finding';
  }
}

export function formatCount(value: number): string {
  return value.toLocaleString();
}

/** "About 3 min left", or null while there is no estimate yet. */
export function formatTimeLeft(seconds: number | null): string | null {
  if (seconds == null) return null;
  if (seconds < 60) return t('Less than a minute left');
  if (seconds < 3600) return t('About {value0} min left', { value0: Math.ceil(seconds / 60) });
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);
  return t('About {value0} h {value1} min left', { value0: hours, value1: minutes });
}

/** The short form for the task bar: "3 min", "1 h 5 min". */
export function formatTimeLeftShort(seconds: number | null): string | null {
  if (seconds == null) return null;
  if (seconds < 60) return t('<1 min');
  if (seconds < 3600) return t('{value0} min', { value0: Math.ceil(seconds / 60) });
  return t('{value0} h {value1} min', {
    value0: Math.floor(seconds / 3600),
    value1: Math.round((seconds % 3600) / 60),
  });
}

export function formatDuration(milliseconds: number): string {
  const seconds = Math.max(1, Math.round(milliseconds / 1000));
  if (seconds < 60) return t('{value0} s', { value0: seconds });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t('{value0} min {value1} s', { value0: minutes, value1: seconds % 60 });
  return t('{value0} h {value1} min', { value0: Math.floor(minutes / 60), value1: minutes % 60 });
}

export function taskTitle(task: ForegroundTask): string {
  switch (taskPhase(task)) {
    case 'finished': return t('Imported “{value0}”', { value0: task.title });
    case 'stopped': return t('Stopped importing “{value0}”', { value0: task.title });
    case 'undone': return t('Undid importing “{value0}”', { value0: task.title });
    case 'removing': return t('Undoing “{value0}”', { value0: task.title });
    default: return t('Importing “{value0}”', { value0: task.title });
  }
}

export function taskSubtitle(task: ForegroundTask): string {
  switch (taskPhase(task)) {
    case 'finding': return t('Finding files…');
    case 'importing':
      return task.rate_per_second != null && task.rate_per_second >= 0.1
        ? t('Adding to library · {value0} files/s', {
          value0: task.rate_per_second >= 10
            ? Math.round(task.rate_per_second)
            : task.rate_per_second.toFixed(1),
        })
        : t('Adding to library');
    case 'paused': return t('Paused');
    case 'removing': return t('Removing the items it added…');
    case 'finished':
      return task.finished_at_ms != null
        ? t('Finished in {value0}', { value0: formatDuration(task.finished_at_ms - task.created_at_ms) })
        : t('Finished');
    case 'stopped': return task.error ?? t('Stopped before every file was imported');
    case 'undone': return t('Removed the {value0} items it added', { value0: formatCount(task.added) });
  }
}

export function taskTone(task: ForegroundTask): 'working' | 'paused' | 'done' | 'warning' {
  const phase = taskPhase(task);
  if (phase === 'paused') return 'paused';
  if (phase === 'finished') return 'done';
  if (phase === 'stopped' || phase === 'undone') return task.error ? 'warning' : 'paused';
  return 'working';
}
