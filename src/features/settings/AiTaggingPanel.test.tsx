import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AiTaggingPanel } from './AiTaggingPanel';

const mocks = vi.hoisted(() => ({
  status: vi.fn(), download: vi.fn(), cancelDownload: vi.fn(), deleteModel: vi.fn(), optimizeModel: vi.fn(),
}));

vi.mock('../../platform/aiTaggerApi', () => ({
  aiTaggerStatus: mocks.status,
  aiTaggerDownloadModel: mocks.download,
  aiTaggerCancelDownload: mocks.cancelDownload,
  aiTaggerDeleteModel: mocks.deleteModel,
  aiTaggerOptimizeModel: mocks.optimizeModel,
}));

const model = {
  slug: 'wd14-swinv2-v3', label: 'WD14', enabled: false, downloaded: false,
  sessionLoaded: false, recommended: true, heavy: false, sizeBytes: 1024 * 1024, dataset: 'test',
  referenceInferenceMs: 17.74,
  optimizationSupported: false, optimized: false,
  downloadedBytes: null, downloadTotalBytes: null,
};

const settings = {
  gridTargetSize: 200, gridViewMode: 'grid', gridSpacing: 'wide' as const, inspectorWidth: 320, colorScheme: 'dark',
  gridSortField: 'added_at', gridSortOrder: 'desc', zoomFactor: null, showTreeGuides: true,
  showSidebarCounts: true, showSidebarInbox: true,
  showSidebarRecentlyViewed: true, showSidebarUncategorized: true, showSidebarUntagged: true,
  showSidebarTagManager: true, showSidebarRandom: true, showSidebarSubscriptions: true,
  showSidebarDuplicates: true, showSidebarQuickAccess: true,
  showSidebarFolders: true, showSidebarSmartFolders: true,
  sidebarDoubleClickAction: 'collapse' as const,
  gridWheelAction: 'scroll' as const, gridDoubleClickAction: 'detail' as const,
  viewerTrackpadGestures: false,
  gridMiddleClickAction: 'new_window' as const, spaceKeyAction: 'quick_look' as const,
  imageRendering: 'smooth' as const, imageDefaultZoom: 'fit' as const,
  showTransparencyGrid: false, videoAutoPlay: true, videoLoop: true,
  notificationPopupsEnabled: true,
  notificationPopupTones: ['error', 'warning', 'info', 'success'] as Array<'error' | 'warning' | 'info' | 'success'>,
  autoImportEnabled: true,
  multiFileImportBehavior: 'ask' as const,
  subscriptionDefaultSchedule: 'daily' as const, subscriptionDefaultPostsPerRun: 100,
  subscriptionDefaultGroupPosts: true,
  showTagGroups: true, starredTags: [], sidebarQuickAccess: [],
  aiTaggerWd14Enabled: false, aiTaggerE621Enabled: false, aiTaggerEva02Enabled: false,
  aiTaggerOppaiOracleEnabled: false, aiTaggerDanbooruTagQueryEnabled: false,
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

async function renderPanel(onSettingsChange = vi.fn()) {
  let result!: ReturnType<typeof render>;
  await act(async () => {
    result = render(<AiTaggingPanel settings={settings} onSettingsChange={onSettingsChange} />);
    await Promise.resolve();
  });
  return result;
}

function setupUser() {
  const user = userEvent.setup();
  return {
    click: (...args: Parameters<typeof user.click>) => act(() => user.click(...args)),
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.status.mockResolvedValue(status(false));
  mocks.download.mockResolvedValue(status(true));
  mocks.cancelDownload.mockResolvedValue(undefined);
  mocks.deleteModel.mockResolvedValue(status(false));
  mocks.optimizeModel.mockResolvedValue(status(true));
});

describe('AiTaggingPanel', () => {
  it('does not expose a model enable toggle before download', async () => {
    await renderPanel();
    await screen.findByRole('button', { name: 'Download' });
    expect(screen.queryByText('Downloaded')).not.toBeInTheDocument();
    expect(screen.getByText('test · 1 MB · ≈18 ms/image')).toBeInTheDocument();
  });

  it('refreshes authoritative model status after download completion', async () => {
    let resolveDownload!: (value: unknown) => void;
    mocks.download.mockReturnValue(new Promise((resolve) => { resolveDownload = resolve; }));
    await renderPanel();
    await screen.findByRole('button', { name: 'Download' });
    await setupUser().click(screen.getByRole('button', { name: 'Download' }));
    expect(mocks.download).toHaveBeenCalledWith(model.slug);
    expect(await screen.findByRole('progressbar')).toBeInTheDocument();
    expect(screen.queryByText('Downloading…')).not.toBeInTheDocument();
    await act(async () => { resolveDownload(status(true)); });
    await waitFor(() => expect(mocks.status.mock.calls.length).toBeGreaterThan(1));
  });

  it('allows multiple downloaded models to be selected independently', async () => {
    const onSettingsChange = vi.fn();
    mocks.status.mockResolvedValue({
      ...status(true),
      models: [
        { ...model, downloaded: true },
        { ...model, slug: 'z3d-e621-convnext', label: 'Z3D', downloaded: true },
      ],
    });
    await renderPanel(onSettingsChange);
    await screen.findByText('Z3D');
    const switches = screen.getAllByRole('switch');
    await setupUser().click(switches[0]);
    await setupUser().click(switches[1]);
    expect(onSettingsChange).toHaveBeenNthCalledWith(1, { aiTaggerWd14Enabled: true });
    expect(onSettingsChange).toHaveBeenNthCalledWith(2, { aiTaggerE621Enabled: true });
  });

  it('exposes the validated OppaiOracle and DanbooruTagQuery runtimes', async () => {
    const onSettingsChange = vi.fn();
    mocks.status.mockResolvedValue({
      ...status(true),
      models: [
        { ...model, slug: 'oppai-oracle-v1-1', label: 'OppaiOracle V1.1', downloaded: true, referenceInferenceMs: 88.58 },
        { ...model, slug: 'danbooru-tag-query-b16', label: 'DanbooruTagQuery B16', downloaded: true, referenceInferenceMs: 13.23 },
      ],
    });
    await renderPanel(onSettingsChange);

    expect(await screen.findByText('OppaiOracle V1.1')).toBeInTheDocument();
    expect(screen.getByText('test · 1 MB · ≈89 ms/image')).toBeInTheDocument();
    expect(screen.getByText('test · 1 MB · ≈13 ms/image')).toBeInTheDocument();

    const switches = screen.getAllByRole('switch');
    await setupUser().click(switches[0]);
    await setupUser().click(switches[1]);
    expect(onSettingsChange).toHaveBeenNthCalledWith(1, { aiTaggerOppaiOracleEnabled: true });
    expect(onSettingsChange).toHaveBeenNthCalledWith(2, { aiTaggerDanbooruTagQueryEnabled: true });
  });

  it('exposes the explicit Mac optimization step for supported downloads', async () => {
    mocks.status.mockResolvedValue({
      ...status(true),
      models: [{ ...model, downloaded: true, optimizationSupported: true }],
    });
    mocks.optimizeModel.mockResolvedValue({
      ...status(true),
      models: [{ ...model, downloaded: true, optimizationSupported: true, optimized: true }],
    });
    await renderPanel();
    await setupUser().click(await screen.findByRole('button', { name: 'Optimize for this Mac' }));
    await waitFor(() => expect(mocks.optimizeModel).toHaveBeenCalledWith(model.slug));
    expect(await screen.findByText('Optimized')).toBeInTheDocument();
  });

  it('cancels an active model operation through the replacement command', async () => {
    let resolveDownload!: (value: unknown) => void;
    mocks.download.mockReturnValue(new Promise((resolve) => { resolveDownload = resolve; }));
    const user = setupUser();
    await renderPanel();
    await user.click(screen.getByRole('button', { name: 'Download' }));
    await user.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(mocks.cancelDownload).toHaveBeenCalledWith(model.slug);
    await act(async () => { resolveDownload(status(false)); });
  });

  it('presents thresholds as percentages without model marketing labels', async () => {
    await renderPanel();
    expect(await screen.findByLabelText('General confidence')).toHaveValue('35');
    expect(screen.getByLabelText('Creator confidence')).toHaveValue('35');
    expect(screen.getByLabelText('Series confidence')).toHaveValue('35');
    expect(screen.queryByLabelText('Artist confidence')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Copyright confidence')).not.toBeInTheDocument();
    expect(screen.getAllByText('35%').length).toBeGreaterThan(0);
    expect(screen.queryByText('Accuracy over speed')).not.toBeInTheDocument();
  });

  it('places the serialized local-processing note below the model list', async () => {
    await renderPanel();
    const note = await screen.findByText('Selected models run locally one after another. Warm single-image reference on an Apple M5 Pro; actual speed varies by device and batch size. Picto never uploads media for AI tagging.');
    const modelList = screen.getByText('WD14').closest('[class*="blockContent"]');

    expect(modelList).not.toContainElement(note);
    expect(modelList?.nextElementSibling).toBe(note);
  });
});
