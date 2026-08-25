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
  inspectorWidth: 320,
  colorScheme: 'dark',
  gridSortField: 'date_added',
  gridSortOrder: 'desc',
  zoomFactor: 1,
  showTreeGuides: true,
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
  const result = render(<Settings />);
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
  return result;
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
    scope_key: '', sort_field: 'date_added', sort_order: 'desc', view_mode: 'waterfall',
    target_size: 220, show_name: true, show_resolution: false, show_extension: false,
    show_label: false, show_item_count: true, thumbnail_fit: 'contain',
  });
});

describe('Settings', () => {
  it('uses the category registry to find and open custom panels', async () => {
    const user = userEvent.setup();
    await renderSettings();

    expect(mocks.getSettings).toHaveBeenCalled();
    expect(mocks.getViewPrefs).toHaveBeenCalled();
    await user.type(screen.getByRole('searchbox', { name: 'Search settings' }), 'theme');
    const generalResult = screen.getByRole('button', { name: /General.*Appearance, zoom, and keyboard layout/i });
    await user.click(generalResult);

    await waitFor(() => expect(screen.getByText('Appearance')).toBeInTheDocument());
    expect(screen.getByText('Keyboard')).toBeInTheDocument();
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
  });

  it('presents keyboard layout in the shared settings-card treatment', async () => {
    await renderSettings();

    expect(screen.getByText('Keyboard')).toBeInTheDocument();
    expect(screen.getByLabelText('Keyboard layout')).toBeInTheDocument();
    expect(screen.getByText(/EU mode adds alternatives/i)).toBeInTheDocument();
  });

  it('previews a draft theme but persists it only when Save Changes is pressed', async () => {
    const user = userEvent.setup();
    const { container } = await renderSettings();

    await user.click(container.querySelector<HTMLButtonElement>('[data-tooltip="Light"]')!);

    expect(document.documentElement.dataset.theme).toBe('light');
    expect(mocks.replaceSettings).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Save Changes' }));
    await waitFor(() => expect(mocks.replaceSettings).toHaveBeenCalledWith(
      expect.objectContaining({ colorScheme: 'light' }),
    ));
  });

  it('uses the same draft transaction for the keyboard preset', async () => {
    const user = userEvent.setup();
    await renderSettings();

    await user.selectOptions(screen.getByLabelText('Keyboard layout'), 'eu');
    expect(getKeyboardPreset()).toBe('eu');
    expect(localStorage.getItem('picto-keyboard-preset')).toBe('us');

    await user.click(screen.getByRole('button', { name: 'Save Changes' }));
    expect(localStorage.getItem('picto-keyboard-preset')).toBe('eu');
  });

  it('keeps a failed save dirty and exposes the failure for retry', async () => {
    const user = userEvent.setup();
    mocks.replaceSettings.mockRejectedValueOnce(new Error('Disk unavailable'));
    const { container } = await renderSettings();

    await user.click(container.querySelector<HTMLButtonElement>('[data-tooltip="Light"]')!);
    await user.click(screen.getByRole('button', { name: 'Save Changes' }));

    expect(await screen.findByRole('status')).toHaveTextContent('Disk unavailable');
    expect(screen.getByRole('button', { name: 'Save Changes' })).toBeEnabled();
    expect(mocks.setViewPrefs).not.toHaveBeenCalled();
  });
});
