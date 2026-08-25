import { addDiagnostic, sourceForTarget, type DiagnosticLevel } from '../features/diagnostics/diagnosticsStore';
import { listen } from '../platform/ipc';

interface ForwardedLog {
  level?: string;
  target?: string;
  message?: string;
  timestamp?: string;
}

function normalizeLevel(level?: string): DiagnosticLevel {
  const normalized = level?.toUpperCase();
  return normalized === 'TRACE' || normalized === 'DEBUG' || normalized === 'WARN' || normalized === 'ERROR'
    ? normalized
    : 'INFO';
}

function accept(raw: ForwardedLog, fallbackSource: 'core' | 'main') {
  const target = raw.target || fallbackSource;
  // Renderer-side IPC timing includes both native and delivery time, so the
  // native picto::ipc trace would duplicate the same call with less context.
  if (target === 'picto::ipc') return;
  addDiagnostic({
    level: normalizeLevel(raw.level),
    source: fallbackSource === 'main' ? 'main' : sourceForTarget(target),
    target,
    message: raw.message || '',
    timestamp: raw.timestamp || new Date().toISOString(),
  });
}

function formatConsoleValue(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value instanceof Error) return value.stack || value.message;
  try { return JSON.stringify(value); } catch { return String(value); }
}

export function startDiagnosticsRuntime(): () => void {
  let stopped = false;
  const disposers: Array<() => void> = [];
  void listen<ForwardedLog>('log', ({ payload }) => accept(payload, 'core')).then((dispose) => {
    if (stopped) dispose(); else disposers.push(dispose);
  });
  void listen<ForwardedLog>('picto:log', ({ payload }) => accept(payload, 'main')).then((dispose) => {
    if (stopped) dispose(); else disposers.push(dispose);
  });

  const onError = (event: ErrorEvent) => addDiagnostic({
    level: 'ERROR', source: 'renderer', target: 'renderer.error',
    message: event.error?.stack || event.message, timestamp: new Date().toISOString(),
  });
  const onRejection = (event: PromiseRejectionEvent) => addDiagnostic({
    level: 'ERROR', source: 'renderer', target: 'renderer.promise',
    message: event.reason?.stack || String(event.reason), timestamp: new Date().toISOString(),
  });
  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onRejection);

  const consoleOriginals = { warn: console.warn, error: console.error };
  const consoleLevels: Record<keyof typeof consoleOriginals, DiagnosticLevel> = {
    warn: 'WARN', error: 'ERROR',
  };
  for (const method of Object.keys(consoleOriginals) as Array<keyof typeof consoleOriginals>) {
    console[method] = (...args: unknown[]) => {
      consoleOriginals[method](...args);
      addDiagnostic({
        level: consoleLevels[method],
        source: 'renderer',
        target: `renderer.console.${method}`,
        message: args.map(formatConsoleValue).join(' '),
        timestamp: new Date().toISOString(),
      });
    };
  }

  return () => {
    stopped = true;
    for (const dispose of disposers) dispose();
    window.removeEventListener('error', onError);
    window.removeEventListener('unhandledrejection', onRejection);
    for (const method of Object.keys(consoleOriginals) as Array<keyof typeof consoleOriginals>) {
      console[method] = consoleOriginals[method];
    }
  };
}
