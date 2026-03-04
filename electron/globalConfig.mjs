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

function getLegacyConfigPaths() {
  const appData = app.getPath('appData');
  return [
    path.join(appData, 'imaginator', 'config.json'),
  ];
}

async function migrateLegacyConfigIfNeeded(configPath) {
  try {
    await fs.access(configPath);
    return false;
  } catch {
    // No picto config yet: try legacy migration.
  }

  for (const legacyPath of getLegacyConfigPaths()) {
    try {
      const raw = await fs.readFile(legacyPath, 'utf-8');
      const parsed = JSON.parse(raw);
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) continue;
      const migrated = { ...DEFAULT_CONFIG, ...parsed };
      await fs.mkdir(path.dirname(configPath), { recursive: true });
      await fs.writeFile(configPath, JSON.stringify(migrated, null, 2), 'utf-8');
      return true;
    } catch {
      // Try next legacy path.
    }
  }

  return false;
}

export async function loadGlobalConfig() {
  const configPath = getConfigPath();
  await migrateLegacyConfigIfNeeded(configPath);
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
  await saveGlobalConfig(config);
}

export function getCachedConfig() {
  return cachedConfig ?? { ...DEFAULT_CONFIG };
}
