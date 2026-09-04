import { app } from 'electron';
import fs from 'node:fs/promises';
import path from 'node:path';

const DEFAULT_CONFIG = {
  libraryHistory: [],
  pinnedLibraries: [],
  cloudLocations: {},
  lastLibrary: null,
  windowState: {
    main: null,
  },
  libraryMeta: {},
};

// App configuration contains host state only. Library preferences belong in
// SQLite; never carry obsolete theme/cloudRoots or arbitrary old keys forward.
function currentConfig(value = {}) {
  return Object.fromEntries(Object.entries(DEFAULT_CONFIG).map(([key, fallback]) => [
    key, value[key] ?? structuredClone(fallback),
  ]));
}

let cachedConfig = null;
let configWriteQueue = Promise.resolve();
let configWriteSequence = 0;

export function getConfigPath() {
  return path.join(app.getPath('appData'), 'picto', 'config.json');
}

export async function loadGlobalConfig() {
  const configPath = getConfigPath();
  let removedObsoleteKeys = false;
  try {
    const raw = await fs.readFile(configPath, 'utf-8');
    const parsed = JSON.parse(raw);
    cachedConfig = currentConfig(parsed);
    removedObsoleteKeys = Object.keys(parsed).some((key) => !Object.hasOwn(DEFAULT_CONFIG, key));
  } catch {
    cachedConfig = currentConfig();
  }
  if (removedObsoleteKeys) {
    try {
      await saveGlobalConfig(cachedConfig);
    } catch (error) {
      console.warn('[config] Could not persist obsolete settings cleanup; will retry next launch', error);
    }
  }
  return cachedConfig;
}

export async function saveGlobalConfig(config) {
  cachedConfig = currentConfig(config);
  const configPath = getConfigPath();
  const contents = JSON.stringify(cachedConfig, null, 2);
  const temporaryPath = `${configPath}.${process.pid}.${configWriteSequence += 1}.tmp`;
  const write = configWriteQueue.catch(() => {}).then(async () => {
    await fs.mkdir(path.dirname(configPath), { recursive: true });
    await fs.writeFile(temporaryPath, contents, 'utf-8');
    await fs.rename(temporaryPath, configPath);
  });
  configWriteQueue = write;
  await write;
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
  return cachedConfig ?? currentConfig();
}
