import {
  connectRemoteLibrary,
  createRemoteLibrary,
  detectShareRoots,
  disconnectSync,
  getSyncStatus,
  listRemoteLibraries,
  syncNow,
} from '../platform/cloudSyncApi';
export type {
  RemoteLibraryInfo,
  ShareRootCandidate,
  SyncCycleResult,
  SyncReport,
  SyncStatus,
} from '../platform/cloudSyncApi';

export const cloudSyncController = {
  getStatus: getSyncStatus,
  detectShareRoots,
  listRemoteLibraries,
  createRemoteLibrary,
  connectRemoteLibrary,
  disconnect: disconnectSync,
  syncNow,
};
