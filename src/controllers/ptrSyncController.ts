import { api, listen, type UnlistenFn } from '#desktop/api';

// Re-export types from central api types for backwards compatibility.
export type {
  PtrSyncProgress,
  PtrBootstrapCounts,
  PtrBootstrapResult,
  PtrBootstrapStatus,
  PtrSyncResult,
  PtrCompactIndexStatus,
  PtrSyncPhaseChangedEvent,
  PtrBootstrapStartedEvent,
  PtrBootstrapProgressEvent,
  PtrBootstrapFinishedEvent,
  PtrBootstrapFailedEvent,
} from '../shared/types/api';
import type {
  PtrSyncProgress,
  PtrBootstrapStatus,
  PtrSyncResult,
  PtrBootstrapStartedEvent,
  PtrBootstrapProgressEvent,
  PtrBootstrapFinishedEvent,
  PtrBootstrapFailedEvent,
} from '../shared/types/api';

export const PtrSyncController = {
  sync(): Promise<unknown> {
    return api.ptr.sync();
  },

  cancelSync(): Promise<void> {
    return api.ptr.cancelSync();
  },

  isSyncing(): Promise<boolean> {
    return api.ptr.isSyncing();
  },

  getSyncProgress(): Promise<PtrSyncProgress | null> {
    return api.ptr.getSyncProgress();
  },

  bootstrapFromHydrusSnapshot(input: {
    snapshot_dir: string;
    ptr_service_id?: number;
    mode: 'dry_run' | 'import';
  }): Promise<unknown> {
    return api.ptr.bootstrapFromSnapshot(input);
  },

  getBootstrapStatus(): Promise<PtrBootstrapStatus> {
    return api.ptr.getBootstrapStatus();
  },

  cancelBootstrap(): Promise<unknown> {
    return api.ptr.cancelBootstrap();
  },

  getSyncPerfBreakdown(): Promise<unknown> {
    return api.ptr.getSyncPerfBreakdown();
  },

  onStarted(handler: () => void): Promise<UnlistenFn> {
    return listen('ptr-sync-started', () => handler());
  },

  onProgress(handler: (progress: PtrSyncProgress) => void): Promise<UnlistenFn> {
    return listen<PtrSyncProgress>('ptr-sync-progress', (event) => handler(event.payload));
  },

  onFinished(handler: (result: PtrSyncResult) => void): Promise<UnlistenFn> {
    return listen<PtrSyncResult>('ptr-sync-finished', (event) => handler(event.payload));
  },

  onBootstrapStarted(handler: (payload: PtrBootstrapStartedEvent) => void): Promise<UnlistenFn> {
    return listen<PtrBootstrapStartedEvent>('ptr-bootstrap-started', (event) => handler(event.payload));
  },

  onBootstrapProgress(handler: (payload: PtrBootstrapProgressEvent) => void): Promise<UnlistenFn> {
    return listen<PtrBootstrapProgressEvent>('ptr-bootstrap-progress', (event) => handler(event.payload));
  },

  onBootstrapFinished(handler: (payload: PtrBootstrapFinishedEvent) => void): Promise<UnlistenFn> {
    return listen<PtrBootstrapFinishedEvent>('ptr-bootstrap-finished', (event) => handler(event.payload));
  },

  onBootstrapFailed(handler: (payload: PtrBootstrapFailedEvent) => void): Promise<UnlistenFn> {
    return listen<PtrBootstrapFailedEvent>('ptr-bootstrap-failed', (event) => handler(event.payload));
  },
};
