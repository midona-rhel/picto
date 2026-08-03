import { invoke } from './ipc';

export interface SyncStatus {
  bound: boolean;
  share_root: string | null;
  library_name: string | null;
  library_uuid: string | null;
  device_id: string;
  pending_ops: number;
}

export interface RemoteLibraryInfo {
  name: string;
  library_uuid: string | null;
  created_at: string | null;
  valid: boolean;
}

export interface SyncReport {
  segments_uploaded: number;
  segments_consumed: number;
  ops_applied: number;
}

export interface SyncCycleResult {
  report: SyncReport;
}

export interface ShareRootCandidate {
  label: string;
  path: string;
  provider: string;
}

export function getSyncStatus(): Promise<SyncStatus> {
  return invoke<SyncStatus>('sync_get_status', {});
}

export function detectShareRoots(): Promise<ShareRootCandidate[]> {
  return invoke<ShareRootCandidate[]>('sync_detect_share_roots', {});
}

export function listRemoteLibraries(shareRoot: string): Promise<RemoteLibraryInfo[]> {
  return invoke<RemoteLibraryInfo[]>('sync_list_remote_libraries', { share_root: shareRoot });
}

export function createRemoteLibrary(shareRoot: string, name: string): Promise<SyncCycleResult> {
  return invoke<SyncCycleResult>('sync_create_remote_library', { share_root: shareRoot, name });
}

export function connectRemoteLibrary(shareRoot: string, name: string): Promise<SyncCycleResult> {
  return invoke<SyncCycleResult>('sync_connect_remote_library', { share_root: shareRoot, name });
}

export function disconnectSync(): Promise<void> {
  return invoke<void>('sync_disconnect', {});
}

export function syncNow(): Promise<SyncCycleResult> {
  return invoke<SyncCycleResult>('sync_now', {});
}
