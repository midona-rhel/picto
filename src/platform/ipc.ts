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

export function invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T> {
  return requireDesktop().api.invoke(command, args ?? {}) as Promise<T>;
}

export function listen<T = unknown>(name: string, handler: (event: { payload: T }) => void): Promise<UnlistenFn> {
  return requireDesktop().events.on(name, (payload: T) => handler({ payload })) as Promise<UnlistenFn>;
}

export function getGpuDiagnostics(): Promise<GpuDiagnostics> {
  return requireDesktop().monitor.gpu() as Promise<GpuDiagnostics>;
}
