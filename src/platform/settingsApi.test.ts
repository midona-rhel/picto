import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.hoisted(() => vi.fn());

vi.mock('./ipc', () => ({ invoke }));

import {
  getSettingsSnapshot,
  getViewPrefs,
  patchSettings,
  replaceSettings,
  resetViewPrefs,
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
    expect(snapshot.value.showSidebarCounts).toBe(true);
    expect(snapshot.value.showSidebarSubscriptions).toBe(true);
    expect(snapshot.value.showSidebarFolders).toBe(true);
    expect(snapshot.value.gridWheelAction).toBe('scroll');
    expect(snapshot.value.viewerTrackpadGestures).toBe(false);
    expect(snapshot.value.gridDoubleClickAction).toBe('detail');
    expect(snapshot.value.gridSpacing).toBe('wide');
    expect(snapshot.value.imageRendering).toBe('smooth');
    expect(snapshot.value.imageDefaultZoom).toBe('fit');
    expect(snapshot.value.showTransparencyGrid).toBe(false);
    expect(snapshot.value.videoAutoPlay).toBe(true);
    expect(snapshot.value.videoLoop).toBe(true);
    expect(snapshot.value.autoImportEnabled).toBe(true);
    expect(snapshot.value.multiFileImportBehavior).toBe('ask');
    expect(snapshot.value.subscriptionDefaultSchedule).toBe('daily');
    expect(snapshot.value.subscriptionDefaultPostsPerRun).toBe(100);
    expect(snapshot.value.subscriptionDefaultGroupPosts).toBe(true);
    expect(snapshot.value.subscriptionInboxItemLimit).toBe(1000);
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

  it('rejects unknown grid spacing values', async () => {
    invoke.mockResolvedValue({ value: { gridSpacing: 'compact' }, revision: 1 });

    await expect(getSettingsSnapshot()).rejects.toThrow('gridSpacing');
  });

  it('rejects unsupported control behavior values', async () => {
    invoke.mockResolvedValue({ value: { gridWheelAction: 'rotate' }, revision: 1 });

    await expect(getSettingsSnapshot()).rejects.toThrow('gridWheelAction');
  });

  it('accepts the macOS media-view trackpad preference', async () => {
    invoke.mockResolvedValue({ value: { viewerTrackpadGestures: true }, revision: 1 });

    await expect(getSettingsSnapshot()).resolves.toMatchObject({
      value: { viewerTrackpadGestures: true },
    });
  });

  it('rejects malformed auto-import preferences', async () => {
    invoke.mockResolvedValue({ value: { autoImportEnabled: 'yes' }, revision: 1 });

    await expect(getSettingsSnapshot()).rejects.toThrow('autoImportEnabled');

    invoke.mockResolvedValue({ value: { multiFileImportBehavior: 'sometimes' }, revision: 1 });
    await expect(getSettingsSnapshot()).rejects.toThrow('multiFileImportBehavior');
  });

  it('rejects invalid subscription defaults', async () => {
    invoke.mockResolvedValue({ value: { subscriptionDefaultPostsPerRun: 0 }, revision: 1 });
    await expect(getSettingsSnapshot()).rejects.toThrow('subscriptionDefaultPostsPerRun');

    invoke.mockResolvedValue({ value: { subscriptionDefaultSchedule: 'hourly' }, revision: 1 });
    await expect(getSettingsSnapshot()).rejects.toThrow('subscriptionDefaultSchedule');

    invoke.mockResolvedValue({ value: { subscriptionInboxItemLimit: 0 }, revision: 1 });
    await expect(getSettingsSnapshot()).rejects.toThrow('subscriptionInboxItemLimit');
  });

  it('uses replacement settings and view commands with their value payloads', async () => {
    invoke.mockResolvedValue({ revision: 3, resources: ['settings'], item_ids: [] });
    const settings = { showTreeGuides: false };
    const viewPatch = { target_size: 300 };

    await patchSettings(settings);
    await replaceSettings(settings as never);
    await setViewPrefs('system:all', viewPatch);
    await resetViewPrefs();

    expect(invoke.mock.calls).toEqual([
      ['settings.patch', { value: settings }],
      ['settings.replace', { value: settings }],
      ['settings.view.patch', { scope: 'system:all', value: viewPatch }],
      ['settings.view.reset', {}],
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
      spacing: null,
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
