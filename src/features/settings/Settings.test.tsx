import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { Settings } from './Settings';

const mocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
  getViewPrefs: vi.fn(),
  setViewPrefs: vi.fn(),
  setZoomFactor: vi.fn(),
}));

vi.mock('../../controllers/settingsController', () => ({
  settingsController: {
    getSettings: mocks.getSettings,
    saveSettings: mocks.saveSettings,
    getViewPrefs: mocks.getViewPrefs,
    setViewPrefs: mocks.setViewPrefs,
    setZoomFactor: mocks.setZoomFactor,
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
  render(<Settings />);
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getSettings.mockResolvedValue(appSettings);
  mocks.getViewPrefs.mockResolvedValue({
    scope_key: '', sort_field: 'date_added', sort_order: 'desc', view_mode: 'waterfall',
    target_size: 220, show_name: true, show_resolution: false, show_extension: false,
    show_label: false, thumbnail_fit: 'contain',
  });
});

describe('Settings', () => {
  it('uses the category registry to find and open custom panels', async () => {
    const user = userEvent.setup();
    await renderSettings();

    expect(mocks.getSettings).toHaveBeenCalled();
    expect(mocks.getViewPrefs).toHaveBeenCalled();
    await user.type(screen.getByRole('searchbox', { name: 'Search settings' }), 'theme');
    const appearanceResult = screen.getByRole('button', { name: /Appearance.*Theme, language, zoom/i });
    await user.click(appearanceResult);

    await waitFor(() => expect(screen.getByText('Grid Defaults')).toBeInTheDocument());
    expect(screen.queryByText(/Search results for/)).not.toBeInTheDocument();
  });

  it('presents keyboard layout in the shared settings-card treatment', async () => {
    await renderSettings();

    expect(screen.getByText('Keyboard')).toBeInTheDocument();
    expect(screen.getByLabelText('Keyboard layout')).toBeInTheDocument();
    expect(screen.getByText(/EU mode adds alternatives/i)).toBeInTheDocument();
  });
});
