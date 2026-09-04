import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

const host = vi.hoisted(() => ({ appData: '' }));
vi.mock('electron', () => ({ app: { getPath: () => host.appData } }));

let config;
beforeEach(async () => {
  host.appData = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-config-test-'));
  vi.resetModules();
  config = await import('./globalConfig.mjs');
});
afterEach(async () => {
  vi.restoreAllMocks();
  await fs.rm(host.appData, { recursive: true, force: true });
});

async function seed(value) {
  await fs.mkdir(path.dirname(config.getConfigPath()), { recursive: true });
  await fs.writeFile(config.getConfigPath(), JSON.stringify(value));
}
async function stored() {
  return JSON.parse(await fs.readFile(config.getConfigPath(), 'utf8'));
}

describe('current host configuration', () => {
  it('removes obsolete settings on disk and preserves all current host state', async () => {
    const current = {
      libraryHistory: ['first.library', 'second.library'],
      pinnedLibraries: ['second.library'],
      lastLibrary: 'first.library',
      cloudLocations: { onedrive: { path: 'cloud' } },
      windowState: { main: { width: 1200, height: 800, maximized: true } },
      libraryMeta: { 'first.library': { name: 'First', icon: 'star' } },
    };
    await seed({ ...current, theme: 'blue', cloudRoots: { old: ['/old'] }, oldSetting: true });
    expect(await config.loadGlobalConfig()).toEqual(current);
    expect(await stored()).toEqual(current);
    expect(await config.loadGlobalConfig()).toEqual(current);
  });

  it('does not rewrite a clean configuration on load', async () => {
    await seed(config.getCachedConfig());
    const write = vi.spyOn(fs, 'writeFile');
    await config.loadGlobalConfig();
    expect(write).not.toHaveBeenCalled();
  });

  it('cannot reintroduce obsolete keys through later saves', async () => {
    await config.saveGlobalConfig({ ...config.getCachedConfig(), theme: 'purple', cloudRoots: {} });
    expect(await stored()).not.toHaveProperty('theme');
    expect(await stored()).not.toHaveProperty('cloudRoots');
    expect(config.getCachedConfig()).toEqual(await stored());
    await config.addLibraryToHistory('current.library');
    await config.togglePinned('current.library');
    expect(await stored()).toMatchObject({
      lastLibrary: 'current.library', libraryHistory: ['current.library'], pinnedLibraries: ['current.library'],
    });
  });

  it('does not share mutable defaults between configurations', () => {
    const first = config.getCachedConfig();
    first.libraryHistory.push('discarded.library');
    first.windowState.main = { width: 100 };
    expect(config.getCachedConfig().libraryHistory).toEqual([]);
    expect(config.getCachedConfig().windowState.main).toBeNull();
  });

  it('reports cleanup write failures without discarding current preferences', async () => {
    await seed({ lastLibrary: 'current.library', theme: 'blue' });
    vi.spyOn(fs, 'writeFile').mockRejectedValueOnce(new Error('read-only config'));
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
    expect(await config.loadGlobalConfig()).toMatchObject({ lastLibrary: 'current.library' });
    expect(config.getCachedConfig()).not.toHaveProperty('theme');
    expect(warning).toHaveBeenCalledWith(expect.stringContaining('cleanup'), expect.any(Error));
    // Failed cleanup leaves the original file intact and is retried on launch.
    expect(await stored()).toHaveProperty('theme', 'blue');
    await config.loadGlobalConfig();
    expect(await stored()).not.toHaveProperty('theme');
  });
});
