import { app } from 'electron';
import fs from 'node:fs/promises';
import path from 'node:path';

const DEFAULT_CONFIG = {
  libraryHistory: [],
  pinnedLibraries: [],
  lastLibrary: null,
  windowState: {
    main: null,
  },
};

let cachedConfig = null;

export function getConfigPath() {
  return path.join(app.getPath('appData'), 'picto', 'config.json');
}

export async function loadGlobalConfig() {
  const configPath = getConfigPath();
  try {
    const raw = await fs.readFile(configPath, 'utf-8');
    cachedConfig = { ...DEFAULT_CONFIG, ...JSON.parse(raw) };
  } catch {
    cachedConfig = { ...DEFAULT_CONFIG };
  }
  return cachedConfig;
}

export async function saveGlobalConfig(config) {
  cachedConfig = config;
  const configPath = getConfigPath();
  await fs.mkdir(path.dirname(configPath), { recursive: true });
  await fs.writeFile(configPath, JSON.stringify(config, null, 2), 'utf-8');
}

export async function addLibraryToHistory(libraryPath) {
  if (!cachedConfig) await loadGlobalConfig();
  const config = cachedConfig;
  // Deduplicate
  config.libraryHistory = config.libraryHistory.filter((p) => p !== libraryPath);
  config.libraryHistory.unshift(libraryPath);
  config.lastLibrary = libraryPath;
  await saveGlobalConfig(config);
}

export async function removeLibraryFromHistory(libraryPath) {
  if (!cachedConfig) await loadGlobalConfig();
  const config = cachedConfig;
  config.libraryHistory = config.libraryHistory.filter((p) => p !== libraryPath);
  config.pinnedLibraries = config.pinnedLibraries.filter((p) => p !== libraryPath);
  if (config.lastLibrary === libraryPath) {
    config.lastLibrary = config.libraryHistory[0] ?? null;
  }
  await saveGlobalConfig(config);
}

export async function togglePinned(libraryPath) {
  if (!cachedConfig) await loadGlobalConfig();
  const config = cachedConfig;
  const idx = config.pinnedLibraries.indexOf(libraryPath);
  if (idx >= 0) {
    config.pinnedLibraries.splice(idx, 1);
  } else {
    config.pinnedLibraries.push(libraryPath);
  }
  await saveGlobalConfig(config);
}

export async function updateLibraryPath(oldPath, newPath) {
  if (!cachedConfig) await loadGlobalConfig();
  const config = cachedConfig;
  const idx = config.libraryHistory.indexOf(oldPath);
  if (idx >= 0) config.libraryHistory[idx] = newPath;
  if (config.lastLibrary === oldPath) config.lastLibrary = newPath;
  const pinIdx = config.pinnedLibraries.indexOf(oldPath);
  if (pinIdx >= 0) config.pinnedLibraries[pinIdx] = newPath;
  if (config.libraryMeta?.[oldPath]) {
    config.libraryMeta[newPath] = config.libraryMeta[oldPath];
    delete config.libraryMeta[oldPath];
  }
  await saveGlobalConfig(config);
}

export function getCachedConfig() {
  return cachedConfig ?? { ...DEFAULT_CONFIG };
}
