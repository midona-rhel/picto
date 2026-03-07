import { notifyError } from './notify';

export function logBestEffortError(operation: string, error: unknown): void {
  console.warn(`[best-effort] ${operation} failed`, error);
}

export function runBestEffort<T>(operation: string, promise: Promise<T>): void {
  void promise.catch((error) => {
    logBestEffortError(operation, error);
  });
}

export function runCriticalAction<T>(
  title: string,
  operation: string,
  promise: Promise<T>,
): void {
  void promise.catch((error) => {
    console.error(`[action] ${operation} failed`, error);
    notifyError(error, title);
  });
}
