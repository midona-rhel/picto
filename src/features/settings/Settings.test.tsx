import { act, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { Settings } from './Settings';
import { getKeyboardPreset, setKeyboardPreset } from '../../shared/lib/shortcuts';
import packageMetadata from '../../../package.json';

const mocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  replaceSettings: vi.fn(),
  getViewPrefs: vi.fn(),
  setViewPrefs: vi.fn(),
  resetViewPrefs: vi.fn(),
  viewPrefsToPatch: (prefs: Record<string, unknown>) => prefs,
  invoke: vi.fn(),
  listen: vi.fn(),
  getUpdateState: vi.fn(),
}));

vi.mock('../../platform/ipc', () => ({
  invoke: mocks.invoke,
  listen: mocks.listen,
}));

vi.mock('../../platform/updateApi', () => ({
  getUpdateState: mocks.getUpdateState,
  checkForUpdates: vi.fn(),
  installUpdate: vi.fn(),
  onUpdateState: vi.fn().mockResolvedValue(() => {}),
  openUpdateRelease: vi.fn(),
}));

vi.mock('../../controllers/settingsController', () => ({
  settingsController: {
    getSettings: mocks.getSettings,
    replaceSettings: mocks.replaceSettings,
    getViewPrefs: mocks.getViewPrefs,
    setViewPrefs: mocks.setViewPrefs,
    resetViewPrefs: mocks.resetViewPrefs,
    viewPrefsToPatch: mocks.viewPrefsToPatch,
  },
}));

const appSettings = {
  gridTargetSize: 220,
  gridViewMode: 'waterfall',
  gridSpacing: 'wide' as const,
  inspectorWidth: 320,
  colorScheme: 'dark',
  gridSortField: 'date_added',
  gridSortOrder: 'desc',
  zoomFactor: 1,
  showTreeGuides: true,
  showSidebarCounts: true,
  showSidebarInbox: true,
  showSidebarRecentlyViewed: true,
  showSidebarUncategorized: true,
  showSidebarUntagged: true,
  showSidebarTagManager: true,
  showSidebarRandom: true,
  showSidebarSubscriptions: true,
  showSidebarDuplicates: true,
  showSidebarQuickAccess: true,
  showSidebarFolders: true,
  showSidebarSmartFolders: true,
  sidebarDoubleClickAction: 'collapse' as const,
  gridWheelAction: 'scroll' as const,
  viewerTrackpadGestures: false,
  gridDoubleClickAction: 'detail' as const,
  gridMiddleClickAction: 'new_window' as const,
  spaceKeyAction: 'quick_look' as const,
  imageRendering: 'smooth' as const,
  imageDefaultZoom: 'fit' as const,
  showTransparencyGrid: false,
  videoAutoPlay: true,
  videoLoop: true,
  notificationPopupsEnabled: true,
  notificationPopupTones: ['error', 'warning', 'info', 'success'] as const,
  autoImportEnabled: true,
  multiFileImportBehavior: 'ask' as const,
  subscriptionDefaultSchedule: 'daily' as const,
  subscriptionDefaultPostsPerRun: 100,
  subscriptionDefaultGroupPosts: true,
  subscriptionInboxItemLimit: 1000,
  showTagGroups: true,
  showTagPrefixes: false,
  starredTags: [],
  sidebarQuickAccess: [],
  aiTaggerWd14Enabled: false,
  aiTaggerE621Enabled: false,
  aiTaggerEva02Enabled: false,
  aiTaggerOppaiOracleEnabled: false,
  aiTaggerAutoOnImport: false,
  aiTaggerWriteRating: false,
  aiThresholdGeneral: 0.35,
  aiThresholdCharacter: 0.35,
  aiThresholdCopyright: 0.35,
  aiThresholdArtist: 0.35,
  aiThresholdSpecies: 0.35,
  aiThresholdRating: 0.35,
};

async function renderSettings() {
  let result!: ReturnType<typeof renderWithProviders>;
  await act(async () => {
    result = renderWithProviders(<Settings />);
    await Promise.resolve();
  });
  return result;
}

function setupUser() {
  const user = userEvent.setup();
  return {
    clear: (...args: Parameters<typeof user.clear>) => act(() => user.clear(...args)),
    click: (...args: Parameters<typeof user.click>) => act(() => user.click(...args)),
    type: (...args: Parameters<typeof user.type>) => act(() => user.type(...args)),
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  Object.defineProperty(window, 'close', { value: vi.fn(), configurable: true });
  localStorage.removeItem('picto:audio-visualization');
  localStorage.removeItem('picto:locale');
  setKeyboardPreset('us');
  mocks.getSettings.mockResolvedValue(appSettings);
  mocks.getUpdateState.mockResolvedValue({
    status: 'idle',
    currentVersion: '43.4.1',
    platform: 'darwin',
    automaticInstall: false,
    version: null,
    releaseName: null,
    releaseDate: null,
    releaseNotes: '',
    releaseUrl: '',
    progress: null,
    error: null,
  });
  mocks.replaceSettings.mockResolvedValue(undefined);
  mocks.setViewPrefs.mockResolvedValue(undefined);
  mocks.resetViewPrefs.mockResolvedValue(undefined);
  mocks.getViewPrefs.mockResolvedValue({
    scope_key: 'grid:defaults', sort_field: 'date_added', sort_order: 'desc', view_mode: 'waterfall',
    target_size: 220, show_name: true, show_resolution: false, show_extension: false,
    show_label: false, show_item_count: true, thumbnail_fit: 'contain',
  });
  mocks.listen.mockResolvedValue(() => {});
  mocks.invoke.mockImplementation((command: string) => {
    if (command === 'library.stats') return Promise.resolve({
      active_items: 12,
      inbox_items: 3,
      trash_items: 2,
      standalone_items: 14,
      collections: 3,
      media_assets: 25,
      image_assets: 20,
      video_assets: 3,
      audio_assets: 1,
      other_assets: 1,
      physical_files: 24,
      original_bytes: 1_572_864,
      tags: 48,
      folders: 6,
      smart_folders: 2,
      subscriptions: 4,
      revision: 7,
    });
    if (command === 'cloud.configuration.get') return Promise.resolve({
      provider: null,
      root_path: null,
      retention: { daily: 30, weekly: 26, yearly: 5 },
    });
    if (command === 'cloud.status.get') return Promise.resolve({
      state: 'disabled',
      message: 'Not configured',
      last_sync_at: null,
      pending_mutations: 0,
      pending_blobs: 0,
      missing_blobs: 0,
    });
    if (command === 'ai.status') return Promise.resolve({
      models: [],
      storageBytes: 0,
      configuredModelSlugs: [],
      thresholds: {
        general: 0.35, character: 0.35, copyright: 0.35,
        artist: 0.35, species: 0.35, rating: 0.35,
      },
      cachedBackend: null,
    });
    return Promise.reject(new Error(`Unexpected command: ${command}`));
  });
});

describe('Settings', () => {
  it('shows the Picto package version in About instead of the Electron runtime version', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'About' }));

    expect(await screen.findByText(`Version ${packageMetadata.version} · macOS`)).toBeInTheDocument();
    expect(screen.queryByText(/43\.4\.1/)).not.toBeInTheDocument();
  });

  it('preloads Cloud and AI Models before navigation without a loading-frame replacement', async () => {
    const user = setupUser();
    await renderSettings();

    await waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith('cloud.configuration.get');
      expect(mocks.invoke).toHaveBeenCalledWith('cloud.status.get');
      expect(mocks.invoke).toHaveBeenCalledWith('ai.status');
    });

    await user.click(screen.getByRole('button', { name: 'Cloud' }));
    expect(screen.getByText('Library Sync')).toBeInTheDocument();
    expect(screen.queryByText(/Loading cloud status/i)).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'AI Models' }));
    expect(screen.getByText('Models')).toBeInTheDocument();
    expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
  });

  it('shows a canonical breakdown of the active library', async () => {
    const user = setupUser();
    await renderSettings();

    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('library.stats'));

    await user.click(screen.getByRole('button', { name: 'Library' }));

    await waitFor(() => expect(screen.getByText('25')).toBeInTheDocument());
    expect(screen.getByText('Overview')).toBeInTheDocument();
    expect(screen.getByText('Media assets')).toBeInTheDocument();
    expect(screen.getByText('1.50 MB')).toBeInTheDocument();
    expect(screen.getByText('Physical files')).toBeInTheDocument();
    expect(screen.getAllByText('Subscriptions')).toHaveLength(2);
  });

  it('uses the category registry to find and open custom panels', async () => {
    const user = setupUser();
    await renderSettings();

    expect(mocks.getSettings).toHaveBeenCalled();
    expect(mocks.getViewPrefs).toHaveBeenCalledWith('grid:defaults');
    await user.type(screen.getByRole('searchbox', { name: 'Search settings' }), 'theme');
    const generalResult = screen.getByRole('button', { name: /General.*Appearance and zoom/i });
    await user.click(generalResult);

    await waitFor(() => expect(screen.getByText('Appearance')).toBeInTheDocument());
    expect(screen.queryByText('Keyboard')).not.toBeInTheDocument();
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
  });

  it('offers every supported language from General settings', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Language' }));
    for (const language of ['English', 'Deutsch', 'Español', 'Português', 'Français', '简体中文', '日本語', 'Suomi']) {
      expect(screen.getAllByText(language).length).toBeGreaterThan(0);
    }
  });

  it('stores compact tag prefixes as a global interface preference', async () => {
    const user = setupUser();
    await renderSettings();

    const hidePrefixes = screen.getByRole('checkbox', { name: 'Hide group prefixes' });
    expect(hidePrefixes).toBeChecked();
    await user.click(hidePrefixes.closest('label')!);
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({ showTagPrefixes: true }),
    ));
  });

  it('presents keyboard layout with the shortcut settings and shared combo box', async () => {
    const user = setupUser();
    await renderSettings();

    expect(screen.queryByLabelText('Keyboard layout')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Shortcuts' }));
    expect(screen.getByText('Keyboard')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Keyboard layout' })).toBeInTheDocument();
    expect(screen.getByText(/EU mode adds alternatives/i)).toBeInTheDocument();
  });

  it('uses sidebar groups and persists behavior and visibility preferences', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByText('Show item counts in the sidebar'));
    await user.click(screen.getByRole('button', { name: 'Sidebar' }));
    expect(screen.getByText('Double Click')).toBeInTheDocument();
    expect(screen.getByText('Show these items in the sidebar:')).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'All' })).toBeDisabled();
    expect(screen.getByRole('checkbox', { name: 'Trash' })).toBeDisabled();
    expect(screen.getByRole('checkbox', { name: 'Inbox' })).toBeEnabled();
    expect(screen.getByText('Folder Tree')).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'Show hierarchy guides' })).toBeEnabled();
    await user.click(screen.getByText('Rename'));
    await user.click(screen.getByRole('checkbox', { name: 'Subscriptions' }).closest('label')!);
    await user.click(screen.getByText('Quick Access'));
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        showSidebarCounts: false,
        showSidebarSubscriptions: false,
        showSidebarQuickAccess: false,
        sidebarDoubleClickAction: 'rename',
      }),
    ));
  });

  it('previews a draft theme but persists it only when Save & Close is pressed', async () => {
    const user = setupUser();
    const { container } = await renderSettings();

    await user.click(container.querySelector<HTMLButtonElement>('[data-tooltip="Light"]')!);

    expect(document.documentElement.dataset.theme).toBe('light');
    expect(mocks.replaceSettings).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Save & Close' }));
    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({ colorScheme: 'light' }),
    ));
  });

  it('uses the same draft transaction for the keyboard preset', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Shortcuts' }));
    await user.click(screen.getByRole('button', { name: 'Keyboard layout' }));
    await user.click(screen.getByRole('option', { name: 'EU (QWERTZ / AZERTY / Nordic)' }));
    expect(getKeyboardPreset()).toBe('eu');
    expect(localStorage.getItem('picto-keyboard-preset')).toBe('us');

    await user.click(screen.getByRole('button', { name: 'Save & Close' }));
    expect(localStorage.getItem('picto-keyboard-preset')).toBe('eu');
  });

  it('persists tight grid spacing through the existing settings transaction', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Controls' }));
    await user.click(screen.getByRole('button', { name: 'Wide' }));
    await user.click(screen.getByRole('option', { name: 'Tight' }));
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({ gridSpacing: 'tight' }),
    ));
  });

  it('resets scope-specific grid choices only when settings are applied', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Controls' }));
    await user.click(screen.getByRole('button', { name: 'Reset all views' }));
    expect(mocks.resetViewPrefs).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.setViewPrefs).toHaveBeenCalledWith(
      'grid:defaults',
      expect.any(Object),
    ));
    expect(mocks.resetViewPrefs).toHaveBeenCalledOnce();
  });

  it('persists the global auto-import switch without replacing per-folder watches', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Auto-Import' }));
    expect(screen.getByText(/watched folders configured in the folder context menu/i)).toBeInTheDocument();
    await user.click(screen.getByRole('switch'));
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({ autoImportEnabled: false }),
    ));
  });

  it('persists how manual multi-file imports are represented', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Auto-Import' }));
    await user.click(screen.getByRole('button', { name: 'Ask every time' }));
    await user.click(screen.getByRole('option', { name: 'Group as collection' }));
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({ multiFileImportBehavior: 'group' }),
    ));
  });

  it('persists defaults used by new subscriptions and source queries', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Subscriptions' }));
    await user.click(screen.getByRole('button', { name: 'Daily' }));
    await user.click(screen.getByRole('option', { name: 'Weekly' }));
    const postsPerRun = screen.getByRole('spinbutton', { name: 'Posts per run' });
    await user.clear(postsPerRun);
    await user.type(postsPerRun, '250');
    const inboxLimit = screen.getByRole('spinbutton', { name: 'Maximum Inbox items' });
    await user.clear(inboxLimit);
    await user.type(inboxLimit, '2000');
    await user.click(screen.getByRole('checkbox', { name: 'Group multi-media posts' }).closest('label')!);
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        subscriptionDefaultSchedule: 'weekly',
        subscriptionDefaultPostsPerRun: 250,
        subscriptionDefaultGroupPosts: false,
        subscriptionInboxItemLimit: 2000,
      }),
    ));
  });

  it('persists mouse and Space-key behavior through the controls panel', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Controls' }));
    await user.click(screen.getByRole('button', { name: 'Scroll grid' }));
    await user.click(screen.getByRole('option', { name: 'Adjust thumbnail size' }));
    await user.click(screen.getByRole('button', { name: 'Quick Look' }));
    await user.click(screen.getByRole('option', { name: 'Scroll page' }));
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({ gridWheelAction: 'zoom', spaceKeyAction: 'scroll' }),
    ));
  });

  it('offers equal-width media-view trackpad controls only on macOS', async () => {
    const originalPlatform = navigator.platform;
    Object.defineProperty(navigator, 'platform', { value: 'MacIntel', configurable: true });
    try {
      const user = setupUser();
      await renderSettings();

      await user.click(screen.getByRole('button', { name: 'Controls' }));
      const wheelSelect = screen.getByRole('button', { name: 'Scroll grid' });
      const gestureSelect = screen.getByRole('button', { name: 'Wheel zoom' });
      const detailSelect = screen.getByRole('button', { name: 'Open Media View' });
      const windowSelect = screen.getByRole('button', { name: 'Open in new window' });
      const spaceSelect = screen.getByRole('button', { name: 'Quick Look' });
      expect([wheelSelect, gestureSelect, detailSelect, windowSelect, spaceSelect].map((element) => element.style.width)).toEqual([
        '220px', '220px', '220px', '220px', '220px',
      ]);

      await user.click(gestureSelect);
      await user.click(screen.getByRole('option', { name: 'Trackpad pan + pinch zoom' }));
      await user.click(screen.getByRole('button', { name: 'Save & Close' }));

      await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
        expect.objectContaining({ viewerTrackpadGestures: true }),
      ));
    } finally {
      Object.defineProperty(navigator, 'platform', { value: originalPlatform, configurable: true });
    }
  });

  it('keeps global controls available without library-scoped grid preferences', async () => {
    const user = setupUser();
    mocks.getViewPrefs.mockRejectedValueOnce(new Error('No library is open'));
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Controls' }));
    expect(screen.getByText('Mouse')).toBeInTheDocument();
    expect(screen.getByText('Keyboard')).toBeInTheDocument();
    expect(screen.queryByText('Grid Defaults')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Scroll grid' }));
    await user.click(screen.getByRole('option', { name: 'Adjust thumbnail size' }));
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({ gridWheelAction: 'zoom' }),
    ));
    expect(mocks.setViewPrefs).not.toHaveBeenCalled();
  });

  it('persists image and video preview behavior through one settings transaction', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Preview' }));
    await user.click(screen.getByRole('button', { name: 'Smooth' }));
    await user.click(screen.getByRole('option', { name: 'Pixelated' }));
    await user.click(screen.getByRole('button', { name: 'Fit to window' }));
    await user.click(screen.getByRole('option', { name: 'Actual size' }));
    await user.click(screen.getByText('Show transparency grid'));
    await user.click(screen.getByText('Autoplay videos'));
    await user.click(screen.getByText('Loop videos'));
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        imageRendering: 'pixelated',
        imageDefaultZoom: 'actual',
        showTransparencyGrid: true,
        videoAutoPlay: false,
        videoLoop: false,
      }),
    ));
  });

  it('keeps a failed save dirty and exposes the failure for retry', async () => {
    const user = setupUser();
    mocks.replaceSettings.mockRejectedValueOnce(new Error('Disk unavailable'));
    const { container } = await renderSettings();

    await user.click(container.querySelector<HTMLButtonElement>('[data-tooltip="Light"]')!);
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    expect(await screen.findByRole('status')).toHaveTextContent('Disk unavailable');
    expect(screen.getByRole('button', { name: 'Save & Close' })).toBeEnabled();
    expect(mocks.setViewPrefs).not.toHaveBeenCalled();
  });
});
