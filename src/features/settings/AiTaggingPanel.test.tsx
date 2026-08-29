import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
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

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
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
  subscriptionInboxItemLimit: 1000,
  subscriptionDefaultGroupPosts: true,
  showTagGroups: true, showTagPrefixes: false, starredTags: [], tagGroupColors: {}, sidebarQuickAccess: [],
  aiTaggerWd14Enabled: false, aiTaggerE621Enabled: false, aiTaggerEva02Enabled: false,
  aiTaggerOppaiOracleEnabled: false, aiTaggerDanbooruTagQueryEnabled: false,
  aiTaggerManualModelSlugs: null,
  aiTaggerAutoOnImport: false, aiTaggerWriteRating: false,
  aiThresholdGeneral: 0.35, aiThresholdCharacter: 0.35, aiThresholdCopyright: 0.35,
  aiThresholdArtist: 0.35, aiThresholdSpecies: 0.35, aiThresholdRating: 0.35,
};

function status(downloaded = false) {
  return {
    models: [{ ...model, downloaded }], storageBytes: downloaded ? 1024 * 1024 : 0, configuredModelSlugs: [],
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
  it('renders a preloaded runtime snapshot on the first frame', () => {
    render(<AiTaggingPanel initialStatus={status(false)} settings={settings} onSettingsChange={vi.fn()} />);

    expect(screen.getByText('Models')).toBeInTheDocument();
    expect(screen.getByText('WD14 SWN')).toBeInTheDocument();
    expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
    expect(mocks.status).not.toHaveBeenCalled();
  });

  it('shows application-wide AI model storage', async () => {
    mocks.status.mockResolvedValue(status(true));
    await renderPanel();

    expect(await screen.findByText('Model storage')).toBeInTheDocument();
    expect(screen.getAllByText('1 MB')).toHaveLength(1);
  });

  it('treats settings as model installation rather than runtime enablement', async () => {
    await renderPanel();
    await screen.findByRole('button', { name: 'Download' });
    expect(screen.queryByText('Downloaded')).not.toBeInTheDocument();
    expect(screen.getByText('test · 1 MB · ≈18 ms/image')).toBeInTheDocument();
    expect(screen.getByText('WD14 SWN').closest('[class*="modelRow"]')?.querySelector('[role="switch"]')).toBeNull();
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

  it('lists multiple downloaded models without implying they stay loaded', async () => {
    mocks.status.mockResolvedValue({
      ...status(true),
      models: [
        { ...model, downloaded: true },
        { ...model, slug: 'z3d-e621-convnext', label: 'Z3D', downloaded: true },
      ],
    });
    await renderPanel();
    await screen.findByText('Z3D');
    expect(screen.getByText('WD14 SWN').closest('[class*="modelRow"]')?.querySelector('[role="switch"]')).toBeNull();
    expect(screen.getByText('Z3D').closest('[class*="modelRow"]')?.querySelector('[role="switch"]')).toBeNull();
  });

  it('selects which downloaded models run automatically on ingestion', async () => {
    const secondModel = { ...model, slug: 'z3d-e621-convnext', label: 'Z3D', downloaded: true };
    mocks.status.mockResolvedValue({
      ...status(true),
      models: [{ ...model, downloaded: true }, secondModel],
    });
    const onSettingsChange = vi.fn();
    await act(async () => {
      render(<AiTaggingPanel
        settings={{ ...settings, aiTaggerAutoOnImport: true, aiTaggerWd14Enabled: true }}
        onSettingsChange={onSettingsChange}
      />);
      await Promise.resolve();
    });

    expect(screen.getByRole('switch', { name: 'Run WD14 SWN on new imports' })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('switch', { name: 'Run Z3D on new imports' })).toHaveAttribute('aria-checked', 'false');
    await setupUser().click(screen.getByRole('switch', { name: 'Run Z3D on new imports' }));
    expect(onSettingsChange).toHaveBeenCalledWith({ aiTaggerE621Enabled: true });
  });

  it('selects a downloaded recommended model when automatic tagging is first enabled', async () => {
    mocks.status.mockResolvedValue(status(true));
    const onSettingsChange = vi.fn();
    await renderPanel(onSettingsChange);

    await setupUser().click(screen.getByRole('switch', { name: 'Auto-tag new imports' }));
    expect(onSettingsChange).toHaveBeenCalledWith({
      aiTaggerAutoOnImport: true,
      aiTaggerWd14Enabled: true,
    });
  });

  it('exposes the validated OppaiOracle and DanbooruTagQuery runtimes', async () => {
    mocks.status.mockResolvedValue({
      ...status(true),
      models: [
        { ...model, slug: 'oppai-oracle-v1-1', label: 'OppaiOracle V1.1', downloaded: true, referenceInferenceMs: 88.58 },
        { ...model, slug: 'danbooru-tag-query-b16', label: 'DanbooruTagQuery B16', downloaded: true, referenceInferenceMs: 13.23 },
      ],
    });
    await renderPanel();

    expect(await screen.findByText('OppaiOracle')).toBeInTheDocument();
    expect(screen.getByText('test · 1 MB · ≈89 ms/image')).toBeInTheDocument();
    expect(screen.getByText('test · 1 MB · ≈13 ms/image')).toBeInTheDocument();

    expect(screen.getByText('OppaiOracle').closest('[class*="modelRow"]')?.querySelector('[role="switch"]')).toBeNull();
    expect(screen.getByText('DTQuery').closest('[class*="modelRow"]')?.querySelector('[role="switch"]')).toBeNull();
  });

  it('exposes the explicit Mac optimization step for supported downloads', async () => {
    let resolveOptimization!: (value: unknown) => void;
    mocks.status.mockResolvedValue({
      ...status(true),
      models: [{ ...model, downloaded: true, optimizationSupported: true }],
    });
    mocks.optimizeModel.mockReturnValue(new Promise((resolve) => { resolveOptimization = resolve; }));
    await renderPanel();
    await setupUser().click(await screen.findByRole('button', { name: 'Optimize for this Mac' }));
    expect(await screen.findByRole('progressbar', { name: 'Optimizing WD14' })).toBeInTheDocument();
    await waitFor(() => expect(mocks.optimizeModel).toHaveBeenCalledWith(model.slug));
    await act(async () => resolveOptimization({
      ...status(true),
      models: [{ ...model, downloaded: true, optimizationSupported: true, optimized: true }],
    }));
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
    const modelList = screen.getByText('WD14 SWN').closest('[class*="blockContent"]');

    expect(modelList).not.toContainElement(note);
    expect(modelList?.nextElementSibling).toBe(note);
  });
});
