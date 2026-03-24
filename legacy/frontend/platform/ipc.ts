export type UnlistenFn = () => void;

function requireDesktop() {
  if (!window.picto?.api?.invoke) {
    throw new Error('Electron desktop API is unavailable. Start via Electron runtime.');
  }
  return window.picto;
}

export { requireDesktop };

export function invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T> {
  return requireDesktop().api.invoke<T>(command, args ?? {});
}

export function listen<T = unknown>(name: string, handler: (event: { payload: T }) => void): Promise<UnlistenFn> {
  return requireDesktop().events.on<T>(name, (payload) => handler({ payload }));
}

export async function emit<T = unknown>(name: string, payload: T): Promise<void> {
  await requireDesktop().events.emit(name, payload);
}

export async function emitTo<T = unknown>(target: string, name: string, payload: T): Promise<void> {
  await requireDesktop().events.emitTo(target, name, payload);
}
