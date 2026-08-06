/** Desktop IPC transport bridge — wraps window.picto preload API. */

export type UnlistenFn = () => void;

export interface GpuDiagnostics {
  hardwareAccelerationEnabled: boolean;
  featureStatus: Record<string, string>;
  info: unknown;
  experimentalFlagsEnabled: boolean;
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
  try {
    return await requireDesktop().api.invoke(command, args ?? {}) as T;
  } catch (error) {
    throw normalizeInvokeError(error);
  }
}

export function listen<T = unknown>(name: string, handler: (event: { payload: T }) => void): Promise<UnlistenFn> {
  return requireDesktop().events.on(name, (payload: T) => handler({ payload })) as Promise<UnlistenFn>;
}
