import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.hoisted(() => vi.fn());

vi.mock('./ipc', () => ({ invoke }));

import {
  getSettingsSnapshot,
  getViewPrefs,
  patchSettings,
  replaceSettings,
  setViewPrefs,
} from './settingsApi';

describe('settings API', () => {
  beforeEach(() => invoke.mockReset());

  it('unwraps and validates replacement settings snapshots', async () => {
    invoke.mockResolvedValue({
      value: { colorScheme: 'light', showTreeGuides: false },
      revision: 12,
    });

    const snapshot = await getSettingsSnapshot();

    expect(invoke).toHaveBeenCalledWith('settings.get', {});
    expect(snapshot.revision).toBe(12);
    expect(snapshot.value.colorScheme).toBe('light');
    expect(snapshot.value.showTreeGuides).toBe(false);
    expect(snapshot.value.showTagGroups).toBe(true);
    expect(snapshot.value.starredTags).toEqual([]);
    expect(snapshot.value.aiTaggerAutoOnImport).toBe(false);
  });

  it('rejects malformed snapshot values instead of silently defaulting them', async () => {
    invoke.mockResolvedValue({ value: { showTreeGuides: 'yes' }, revision: 1 });

    await expect(getSettingsSnapshot()).rejects.toThrow('showTreeGuides');
  });

  it('rejects malformed starred tag preferences', async () => {
    invoke.mockResolvedValue({ value: { starredTags: ['creator:alice', 2] }, revision: 1 });

    await expect(getSettingsSnapshot()).rejects.toThrow('starredTags');
  });

  it('uses replacement settings and view commands with their value payloads', async () => {
    invoke.mockResolvedValue({ revision: 3, resources: ['settings'], item_ids: [] });
    const settings = { showTreeGuides: false };
    const viewPatch = { target_size: 300 };

    await patchSettings(settings);
    await replaceSettings(settings as never);
    await setViewPrefs('system:all', viewPatch);

    expect(invoke.mock.calls).toEqual([
      ['settings.patch', { value: settings }],
      ['settings.replace', { value: settings }],
      ['settings.view.patch', { scope: 'system:all', value: viewPatch }],
    ]);
  });

  it('returns view preferences with the requested scope and validated fields', async () => {
    invoke.mockResolvedValue({
      value: { view_mode: 'grid', target_size: 240, show_name: true },
      revision: 8,
    });

    await expect(getViewPrefs('folder:4')).resolves.toEqual({
      scope_key: 'folder:4',
      sort_field: null,
      sort_order: null,
      view_mode: 'grid',
      target_size: 240,
      show_name: true,
      show_resolution: null,
      show_extension: null,
      show_label: null,
      show_item_count: null,
      thumbnail_fit: null,
      show_subfolders: null,
    });
    expect(invoke).toHaveBeenCalledWith('settings.view.get', { scope: 'folder:4' });
  });
});
