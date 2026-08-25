import { useSyncExternalStore } from 'react';

export type AudioVisualizationMode = 'none' | 'spectrum' | 'oscilloscope' | 'orbit';

export const AUDIO_VISUALIZATION_OPTIONS: Array<{
  value: AudioVisualizationMode;
  label: string;
}> = [
  { value: 'none', label: 'None' },
  { value: 'spectrum', label: 'Spectrum' },
  { value: 'oscilloscope', label: 'Oscilloscope' },
  { value: 'orbit', label: 'Orbit' },
];

const STORAGE_KEY = 'picto:audio-visualization';
const CHANGE_EVENT = 'picto:audio-visualization-changed';
const DEFAULT_MODE: AudioVisualizationMode = 'spectrum';
const VALID_MODES = new Set<AudioVisualizationMode>(
  AUDIO_VISUALIZATION_OPTIONS.map(({ value }) => value),
);

function storedMode(): AudioVisualizationMode {
  const stored = globalThis.localStorage?.getItem(STORAGE_KEY) as AudioVisualizationMode | null;
  return stored && VALID_MODES.has(stored) ? stored : DEFAULT_MODE;
}

let activeMode = storedMode();

export function getAudioVisualizationMode(): AudioVisualizationMode {
  return activeMode;
}

export function setAudioVisualizationMode(mode: AudioVisualizationMode, persist = true): void {
  activeMode = mode;
  if (persist) globalThis.localStorage?.setItem(STORAGE_KEY, mode);
  globalThis.dispatchEvent?.(new Event(CHANGE_EVENT));
}

function subscribe(onChange: () => void): () => void {
  const onStorage = (event: StorageEvent) => {
    if (event.key === STORAGE_KEY) onChange();
  };
  globalThis.addEventListener?.(CHANGE_EVENT, onChange);
  globalThis.addEventListener?.('storage', onStorage as EventListener);
  return () => {
    globalThis.removeEventListener?.(CHANGE_EVENT, onChange);
    globalThis.removeEventListener?.('storage', onStorage as EventListener);
  };
}

export function useAudioVisualizationMode(): AudioVisualizationMode {
  return useSyncExternalStore(subscribe, getAudioVisualizationMode, () => DEFAULT_MODE);
}
