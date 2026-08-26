import type { DiagnosticEntry } from './diagnosticsStore';

export interface SupportWorker {
  id: string;
  label: string;
  state: string;
  detail: string;
  active: number;
  queued: number;
  attention: number;
}

export function formatDiagnosticEntry(entry: DiagnosticEntry): string {
  const duration = entry.durationMs == null
    ? ''
    : entry.nativeDurationMs == null
      ? `${entry.durationMs.toFixed(1)} ms`
      : `${entry.nativeDurationMs.toFixed(1)} ms native / ${entry.durationMs.toFixed(1)} ms total`;
  return [entry.timestamp, entry.level, entry.source, entry.target, duration, entry.message].join('\t');
}

export function scrubSupportText(value: string): string {
  return value
    .replace(/\/(?:Users|home)\/[^/\s]+/g, '~')
    .replace(/[A-Za-z]:\\Users\\[^\\\s]+/g, '~')
    .replace(/\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/gi, '<email>')
    .replace(/\b(Bearer\s+)[A-Za-z0-9._~+/=-]+/gi, '$1<redacted>')
    .replace(/([?&](?:access_token|api_key|apikey|authorization|cookie|password|refresh_token|token)=)[^&\s]+/gi, '$1<redacted>');
}

export function buildSupportReport(
  entries: DiagnosticEntry[],
  workers: SupportWorker[],
  environment: { userAgent: string; language: string },
): string {
  const lines = [
    'Picto support report',
    `Created\t${new Date().toISOString()}`,
    `Runtime\t${environment.userAgent}`,
    `Language\t${environment.language}`,
    '',
    'Workers',
    'ID\tName\tState\tActive\tQueued\tAttention\tDetail',
    ...workers.map((worker) => [
      worker.id, worker.label, worker.state, worker.active, worker.queued, worker.attention, worker.detail,
    ].join('\t')),
    '',
    'Logs',
    'Timestamp\tLevel\tSource\tTarget\tDuration\tMessage',
    ...entries.map(formatDiagnosticEntry),
    '',
  ];
  return scrubSupportText(lines.join('\n'));
}
