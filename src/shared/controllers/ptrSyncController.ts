import { api } from '#desktop/api';

// Re-export types from central api types for backwards compatibility.
export type {
  PtrSyncProgress,
  PtrBootstrapCounts,
  PtrBootstrapResult,
  PtrBootstrapStatus,
  PtrSyncResult,
  PtrCompactIndexStatus,
} from '../types/api';
import type {
  PtrSyncProgress,
  PtrBootstrapStatus,
} from '../types/api';

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
};
