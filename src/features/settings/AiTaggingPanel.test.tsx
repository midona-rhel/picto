import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AiTaggingPanel } from './AiTaggingPanel';

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  download: vi.fn(),
  cancelDownload: vi.fn(),
  deleteModel: vi.fn(),
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
  taskState: { autoTag: null, downloads: {} as Record<string, any> },
}));

vi.mock('../../platform/aiTaggerApi', () => ({
  aiTaggerStatus: mocks.status,
  aiTaggerDownloadModel: mocks.download,
  aiTaggerCancelDownload: mocks.cancelDownload,
  aiTaggerDeleteModel: mocks.deleteModel,
}));

vi.mock('../../controllers/settingsController', () => ({
  settingsController: {
    getSettings: mocks.getSettings,
    saveSettings: mocks.saveSettings,
  },
}));

vi.mock('../../runtime/aiTaggerTasks', () => ({
  useAiTaggerTasks: () => mocks.taskState,
}));

const model = {
  slug: 'wd14-swinv2-v3',
  label: 'WD14',
  enabled: false,
  downloaded: false,
  recommended: true,
  heavy: false,
  sizeBytes: 1024 * 1024,
  dataset: 'test',
};

const settings = {
  gridTargetSize: 200,
  gridViewMode: 'grid',
  inspectorWidth: 320,
  colorScheme: 'dark',
  gridSortField: 'added_at',
  gridSortOrder: 'desc',
  zoomFactor: null,
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

function status(downloaded = false) {
  return {
    models: [{ ...model, downloaded }],
    gpuBackend: null,
    availableModels: [],
    hardware: { memoryBytes: 8e9, logicalCores: 8, cpuModel: 'Test CPU', executionProvider: 'CPU' },
  };
}

async function renderPanel() {
  const result = render(<AiTaggingPanel />);
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  return result;
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.taskState = { autoTag: null, downloads: {} };
  mocks.status.mockResolvedValue(status(false));
  mocks.getSettings.mockResolvedValue(settings);
  mocks.saveSettings.mockResolvedValue(undefined);
  mocks.download.mockResolvedValue(undefined);
  mocks.cancelDownload.mockResolvedValue(undefined);
  mocks.deleteModel.mockResolvedValue(undefined);
});

describe('AiTaggingPanel', () => {
  it('does not expose a model enable toggle before download', async () => {
    await renderPanel();

    await screen.findByText('Download 1 MB');
    expect(screen.queryByText('Downloaded')).not.toBeInTheDocument();
    expect(screen.getAllByRole('button')).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'Download 1 MB' })).toBeInTheDocument();
  });

  it('awaits download completion and refreshes model status', async () => {
    let resolveDownload!: () => void;
    mocks.download.mockReturnValue(new Promise<void>((resolve) => { resolveDownload = resolve; }));
    await renderPanel();
    await screen.findByRole('button', { name: 'Download 1 MB' });
    const statusCallsBeforeDownload = mocks.status.mock.calls.length;

    await userEvent.setup().click(screen.getByRole('button', { name: 'Download 1 MB' }));
    expect(mocks.download).toHaveBeenCalledWith(model.slug);
    expect(mocks.status).toHaveBeenCalledTimes(statusCallsBeforeDownload);

    await act(async () => {
      resolveDownload();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await waitFor(() => expect(mocks.status.mock.calls.length).toBeGreaterThan(statusCallsBeforeDownload));
  });

  it('offers cancellation while a model download is active', async () => {
    const user = userEvent.setup();
    const view = await renderPanel();
    await screen.findByRole('button', { name: 'Download 1 MB' });

    mocks.taskState = {
      autoTag: null,
      downloads: {
        [model.slug]: {
          task_id: `model_download:${model.slug}`,
          kind: 'model_download',
          status: 'running',
          progress: { done: 1, total: 2 },
        },
      },
    };
    view.rerender(<AiTaggingPanel />);
    expect(await screen.findByRole('button', { name: 'Cancel' })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(mocks.cancelDownload).toHaveBeenCalledWith(model.slug);
  });
});
