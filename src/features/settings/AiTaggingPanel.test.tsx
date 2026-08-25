import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AiTaggingPanel } from './AiTaggingPanel';

const mocks = vi.hoisted(() => ({
  status: vi.fn(), download: vi.fn(), cancelDownload: vi.fn(), deleteModel: vi.fn(),
}));

vi.mock('../../platform/aiTaggerApi', () => ({
  aiTaggerStatus: mocks.status,
  aiTaggerDownloadModel: mocks.download,
  aiTaggerCancelDownload: mocks.cancelDownload,
  aiTaggerDeleteModel: mocks.deleteModel,
}));

const model = {
  slug: 'wd14-swinv2-v3', label: 'WD14', enabled: false, downloaded: false,
  sessionLoaded: false, recommended: true, heavy: false, sizeBytes: 1024 * 1024, dataset: 'test',
};

const settings = {
  gridTargetSize: 200, gridViewMode: 'grid', inspectorWidth: 320, colorScheme: 'dark',
  gridSortField: 'added_at', gridSortOrder: 'desc', zoomFactor: null, showTreeGuides: true,
  showTagGroups: true, starredTags: [], sidebarQuickAccess: [],
  aiTaggerWd14Enabled: false, aiTaggerE621Enabled: false, aiTaggerEva02Enabled: false,
  aiTaggerAutoOnImport: false, aiTaggerWriteRating: false,
  aiThresholdGeneral: 0.35, aiThresholdCharacter: 0.35, aiThresholdCopyright: 0.35,
  aiThresholdArtist: 0.35, aiThresholdSpecies: 0.35, aiThresholdRating: 0.35,
};

function status(downloaded = false) {
  return {
    models: [{ ...model, downloaded }], configuredModelSlugs: [],
    thresholds: { general: 0.35, character: 0.35, copyright: 0.35, artist: 0.35, species: 0.35, rating: 0.35 },
    cachedBackend: null,
  };
}

async function renderPanel() {
  const result = render(<AiTaggingPanel settings={settings} onSettingsChange={vi.fn()} />);
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
  return result;
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.status.mockResolvedValue(status(false));
  mocks.download.mockResolvedValue(status(true));
  mocks.cancelDownload.mockResolvedValue(undefined);
  mocks.deleteModel.mockResolvedValue(status(false));
});

describe('AiTaggingPanel', () => {
  it('does not expose a model enable toggle before download', async () => {
    await renderPanel();
    await screen.findByRole('button', { name: 'Download' });
    expect(screen.queryByText('Downloaded')).not.toBeInTheDocument();
    expect(screen.getByText('test · 1 MB · Recommended')).toBeInTheDocument();
  });

  it('refreshes authoritative model status after download completion', async () => {
    let resolveDownload!: (value: unknown) => void;
    mocks.download.mockReturnValue(new Promise((resolve) => { resolveDownload = resolve; }));
    await renderPanel();
    await screen.findByRole('button', { name: 'Download' });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Download' }));
    expect(mocks.download).toHaveBeenCalledWith(model.slug);
    expect(await screen.findByText('Downloading…')).toBeInTheDocument();
    await act(async () => { resolveDownload(status(true)); });
    await waitFor(() => expect(mocks.status.mock.calls.length).toBeGreaterThan(1));
  });

  it('cancels an active model operation through the replacement command', async () => {
    let resolveDownload!: (value: unknown) => void;
    mocks.download.mockReturnValue(new Promise((resolve) => { resolveDownload = resolve; }));
    const user = userEvent.setup();
    await renderPanel();
    await user.click(screen.getByRole('button', { name: 'Download' }));
    await user.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(mocks.cancelDownload).toHaveBeenCalledWith(model.slug);
    resolveDownload(status(false));
  });

  it('presents thresholds as percentages without model marketing labels', async () => {
    await renderPanel();
    expect(await screen.findByLabelText('General confidence')).toHaveValue('35');
    expect(screen.getAllByText('35%').length).toBeGreaterThan(0);
    expect(screen.queryByText('Accuracy over speed')).not.toBeInTheDocument();
  });

  it('places the local-processing note below the model list', async () => {
    await renderPanel();
    const note = await screen.findByText('Models run locally. Picto never uploads media for AI tagging.');
    const modelList = screen.getByText('WD14').closest('[class*="blockContent"]');

    expect(modelList).not.toContainElement(note);
    expect(modelList?.nextElementSibling).toBe(note);
  });
});
