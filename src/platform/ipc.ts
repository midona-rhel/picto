/** Desktop IPC transport bridge — wraps window.picto preload API. */

import { recordIpcCall } from '../features/diagnostics/diagnosticsStore';

export type UnlistenFn = () => void;

export interface GpuDiagnostics {
  hardwareAccelerationEnabled: boolean;
  featureStatus: Record<string, string>;
  info: unknown;
  experimentalFlagsEnabled: boolean;
}

interface CoreJsonEnvelope {
  __pictoCoreJson: string;
  __pictoNativeMs: number;
}

function decodeInvokeResult<T>(result: unknown): { value: T; nativeDurationMs?: number } {
  if (result && typeof result === 'object' && '__pictoCoreJson' in result) {
    const envelope = result as CoreJsonEnvelope;
    return {
      value: JSON.parse(envelope.__pictoCoreJson) as T,
      nativeDurationMs: envelope.__pictoNativeMs,
    };
  }
  return { value: result as T };
}

function requireDesktop() {
  if (!(window as any).picto?.api?.invoke) {
    throw new Error('Electron desktop API is unavailable.');
  }
  return (window as any).picto;
}

export function normalizeInvokeError(error: unknown): Error {
  const message = (error instanceof Error ? error.message : String(error))
    .replace(/^Error invoking remote method ['"]picto:invoke['"]:\s*/i, '')
    .replace(/^Error:\s*/i, '')
    .trim();
  return new Error(message || 'The request failed.');
}

export async function invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T> {
  const started = performance.now();
  try {
    const transportResult = await requireDesktop().api.invoke(command, args ?? {});
    const result = decodeInvokeResult<T>(transportResult);
    recordIpcCall(command, performance.now() - started, undefined, result.nativeDurationMs);
    return result.value;
  } catch (error) {
    const normalized = normalizeInvokeError(error);
    recordIpcCall(command, performance.now() - started, normalized);
    throw normalized;
  }
}

export function listen<T = unknown>(name: string, handler: (event: { payload: T }) => void): Promise<UnlistenFn> {
  return requireDesktop().events.on(name, (payload: T) => handler({ payload })) as Promise<UnlistenFn>;
}
