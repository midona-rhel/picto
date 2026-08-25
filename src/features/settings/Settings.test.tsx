import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { Settings } from './Settings';
import { getKeyboardPreset, setKeyboardPreset } from '../../shared/lib/shortcuts';

const mocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  replaceSettings: vi.fn(),
  getViewPrefs: vi.fn(),
  setViewPrefs: vi.fn(),
  viewPrefsToPatch: (prefs: Record<string, unknown>) => prefs,
}));

vi.mock('../../controllers/settingsController', () => ({
  settingsController: {
    getSettings: mocks.getSettings,
    replaceSettings: mocks.replaceSettings,
    getViewPrefs: mocks.getViewPrefs,
    setViewPrefs: mocks.setViewPrefs,
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
  subscriptionDefaultSchedule: 'daily' as const,
  subscriptionDefaultPostsPerRun: 100,
  subscriptionDefaultGroupPosts: true,
  showTagGroups: true,
  starredTags: [],
  sidebarQuickAccess: [],
  aiTaggerWd14Enabled: false,
  aiTaggerE621Enabled: false,
  aiTaggerEva02Enabled: false,
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
  let result!: ReturnType<typeof render>;
  await act(async () => {
    result = render(<Settings />);
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
  setKeyboardPreset('us');
  mocks.getSettings.mockResolvedValue(appSettings);
  mocks.replaceSettings.mockResolvedValue(undefined);
  mocks.setViewPrefs.mockResolvedValue(undefined);
  mocks.getViewPrefs.mockResolvedValue({
    scope_key: 'system:active', sort_field: 'date_added', sort_order: 'desc', view_mode: 'waterfall',
    target_size: 220, show_name: true, show_resolution: false, show_extension: false,
    show_label: false, show_item_count: true, thumbnail_fit: 'contain',
  });
});

describe('Settings', () => {
  it('uses the category registry to find and open custom panels', async () => {
    const user = setupUser();
    await renderSettings();

    expect(mocks.getSettings).toHaveBeenCalled();
    expect(mocks.getViewPrefs).toHaveBeenCalledWith('system:active');
    await user.type(screen.getByRole('searchbox', { name: 'Search settings' }), 'theme');
    const generalResult = screen.getByRole('button', { name: /General.*Appearance and zoom/i });
    await user.click(generalResult);

    await waitFor(() => expect(screen.getByText('Appearance')).toBeInTheDocument());
    expect(screen.queryByText('Keyboard')).not.toBeInTheDocument();
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
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

  it('persists defaults used by new subscriptions and source queries', async () => {
    const user = setupUser();
    await renderSettings();

    await user.click(screen.getByRole('button', { name: 'Subscriptions' }));
    await user.click(screen.getByRole('button', { name: 'Daily' }));
    await user.click(screen.getByRole('option', { name: 'Weekly' }));
    const postsPerRun = screen.getByRole('spinbutton', { name: 'Posts per run' });
    await user.clear(postsPerRun);
    await user.type(postsPerRun, '250');
    await user.click(screen.getByRole('checkbox', { name: 'Group multi-media posts' }).closest('label')!);
    await user.click(screen.getByRole('button', { name: 'Save & Close' }));

    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        subscriptionDefaultSchedule: 'weekly',
        subscriptionDefaultPostsPerRun: 250,
        subscriptionDefaultGroupPosts: false,
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
