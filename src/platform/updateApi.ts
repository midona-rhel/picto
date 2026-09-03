export type UpdateStatus = 'idle' | 'checking' | 'current' | 'available' | 'downloading' | 'downloaded' | 'installed' | 'error' | 'unavailable';

export interface UpdateState {
  status: UpdateStatus;
  currentVersion: string;
  platform: string;
  automaticInstall: boolean;
  version: string | null;
  releaseName: string | null;
  releaseDate: string | null;
  releaseNotes: string;
  releaseUrl: string;
  progress: { percent: number; transferred: number; total: number } | null;
  error: string | null;
}

function updates() {
  const api = (window as any).picto?.updates;
  if (!api) throw new Error('Desktop update service is unavailable.');
  return api;
}

export const getUpdateState = (): Promise<UpdateState> => updates().state();
export const checkForUpdates = (): Promise<UpdateState> => updates().check();
export const installUpdate = (): Promise<void> => updates().install();
export const openUpdateRelease = (): Promise<void> => updates().openRelease();
export const acknowledgeInstalledUpdate = (): Promise<UpdateState> => updates().acknowledgeInstalled();
export const onUpdateState = (handler: (state: UpdateState) => void): Promise<() => void> => Promise.resolve(updates().onState(handler));
