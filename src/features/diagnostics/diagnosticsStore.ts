import { useSyncExternalStore } from 'react';

export type DiagnosticLevel = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
export type DiagnosticSource = 'core' | 'main' | 'renderer' | 'ipc';

export interface DiagnosticEntry {
  id: number;
  level: DiagnosticLevel;
  source: DiagnosticSource;
  target: string;
  message: string;
  timestamp: string;
  durationMs?: number;
  nativeDurationMs?: number;
}

const MAX_ENTRIES = 2_000;
let entries: DiagnosticEntry[] = [];
let nextId = 1;
const subscribers = new Set<() => void>();

function emit() {
  for (const subscriber of subscribers) subscriber();
}

export function addDiagnostic(entry: Omit<DiagnosticEntry, 'id'>) {
  entries = [...entries, { ...entry, id: nextId++ }].slice(-MAX_ENTRIES);
  emit();
}

export function clearDiagnostics() {
  entries = [];
  emit();
}

export function getDiagnosticsSnapshot() {
  return entries;
}

export function useDiagnostics() {
  return useSyncExternalStore(
    (subscriber) => {
      subscribers.add(subscriber);
      return () => subscribers.delete(subscriber);
    },
    getDiagnosticsSnapshot,
  );
}

export function recordIpcCall(
  command: string,
  durationMs: number,
  error?: unknown,
  nativeDurationMs?: number,
) {
  // The diagnostics panel polls this command. Logging the poll would make the
  // observer its own largest source of noise.
  if (command === 'diagnostics.snapshot') return;
  const failed = error !== undefined;
  const errorMessage = error instanceof Error ? error.message : String(error);
  // Supersession is expected while scrolling. Keep useful slow-work timing
  // without reporting normal query cancellation as an application failure.
  if (command === 'items.window' && failed && errorMessage === 'query superseded') {
    if (durationMs >= 16) addDiagnostic({
      level: 'DEBUG', source: 'ipc', target: command,
      message: 'Superseded by a newer grid request',
      timestamp: new Date().toISOString(), durationMs,
    });
    return;
  }
  // Cloud status polling starts before the library gate has finished opening a
  // library. That unavailable state is expected and retried by the caller.
  if (command === 'cloud.status.get' && failed && /^No library is open\b/i.test(errorMessage)) return;
  // Successful, routine IPC is not actionable support information. Keep slow
  // calls and failures; native mutation audit events cover user-visible work.
  if (!failed && durationMs < 16) return;
  const rendererDurationMs = nativeDurationMs == null ? null : Math.max(0, durationMs - nativeDurationMs);
  addDiagnostic({
    level: failed ? 'ERROR' : durationMs >= 100 ? 'WARN' : 'DEBUG',
    source: 'ipc',
    target: command,
    message: failed
      ? errorMessage
      : nativeDurationMs == null
        ? 'Completed'
        : `Round trip ${durationMs.toFixed(1)} ms · delivery/render ${rendererDurationMs!.toFixed(1)} ms`,
    timestamp: new Date().toISOString(),
    durationMs,
    nativeDurationMs,
  });
}

export function sourceForTarget(target: string): DiagnosticSource {
  if (target.startsWith('electron') || target.startsWith('main')) return 'main';
  if (target.startsWith('renderer')) return 'renderer';
  return 'core';
}
