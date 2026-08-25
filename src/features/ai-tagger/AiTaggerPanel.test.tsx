import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Provider, createStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { AiTaggerPanel } from './AiTaggerPanel';

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  predict: vi.fn(),
  apply: vi.fn(),
  announce: vi.fn(),
  details: vi.fn(),
  portalAtom: undefined as any,
  targetAtom: undefined as any,
}));

vi.mock('../../platform/aiTaggerApi', () => ({
  aiTaggerStatus: mocks.status,
  aiTagPredict: mocks.predict,
  aiTagApply: mocks.apply,
}));

vi.mock('../../runtime/historyRuntime', () => ({
  announceUndoableMutation: mocks.announce,
}));

vi.mock('../../controllers/viewerController', () => ({
  viewerController: { getItemDetails: mocks.details },
}));

vi.mock('../../state/portals', async () => {
  const { atom: makeAtom } = await import('jotai');
  const portalAtom = makeAtom({ open: false, anchor: null });
  mocks.portalAtom = portalAtom;
  return { aiTaggerPortalAtom: portalAtom };
});

vi.mock('../../state/selection', async () => {
  const { atom: makeAtom } = await import('jotai');
  const targetAtom = makeAtom<any>(null);
  mocks.targetAtom = targetAtom;
  return { selectionTargetAtom: targetAtom };
});

vi.mock('../../shared/ui/OverlayShell', () => ({
  OverlayShell: ({ open, onClose, header, footer, children }: any) => open ? (
    <div role="dialog" aria-label="AI tagger">
      <button type="button" aria-label="Close" onClick={onClose} />
      <div>{header}</div><div>{children}</div><div>{footer}</div>
    </div>
  ) : null,
}));

const model = {
  slug: 'wd14-swinv2-v3', label: 'WD14', enabled: true, downloaded: true,
  sessionLoaded: false, recommended: true, heavy: false, sizeBytes: 1, dataset: 'test',
};

const prediction = (mediaItemId: number, tag = 'cat', confidence = 0.8) => ({
  mediaItemId,
  error: null,
  predictions: [{ tag, namespace: 'character', confidence, model: model.slug }],
});

const details = (itemId: number) => ({
  item_id: itemId,
  kind: 'media',
  lifecycle: 'active',
  label: null,
  cover_media_item_id: null,
  folder_ids: [],
  aggregate_tags: [],
  revision: 1,
  media: [{
    media_item_id: itemId,
    file_hash: `hash-${itemId}`,
    mime_type: 'image/jpeg',
    dominant_color_hex: '#202020',
    dominant_colors: ['#202020'],
    size_bytes: 100,
    pixel_width: 100,
    pixel_height: 100,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    name: `Image ${itemId}`,
    notes: null,
    rating: null,
    source_urls: [],
    captured_at: null,
    imported_at: '2026-01-01T00:00:00Z',
    position: 0,
    tags: [],
  }],
});

async function renderPanel(itemIds = [1]) {
  const store = createStore();
  store.set(mocks.portalAtom, { open: true, anchor: null });
  store.set(mocks.targetAtom, { kind: 'explicit', item_ids: itemIds });
  let result!: ReturnType<typeof renderWithProviders>;
  await act(async () => {
    result = renderWithProviders(<Provider store={store}><AiTaggerPanel /></Provider>);
    await new Promise((resolve) => setTimeout(resolve, 10));
  });
  return { ...result, store };
}

function setupUser() {
  const user = userEvent.setup();
  return {
    click: (...args: Parameters<typeof user.click>) => act(() => user.click(...args)),
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.status.mockResolvedValue({
    models: [model], configuredModelSlugs: [model.slug],
    thresholds: { general: 0.35, character: 0.35, copyright: 0.35, artist: 0.35, species: 0.35, rating: 0.35 },
    cachedBackend: null,
  });
  mocks.predict.mockResolvedValue({ predictions: [prediction(1)], thresholds: { general: 0.35, character: 0.35 } });
  mocks.details.mockImplementation(async (itemId: number) => details(itemId));
  mocks.apply.mockResolvedValue({ revision: 1, resources: ['tags'], item_ids: [1] });
  mocks.announce.mockResolvedValue(undefined);
});

describe('AiTaggerPanel', () => {
  it('uses replacement numeric item IDs for prediction and apply', async () => {
    mocks.predict.mockImplementation(async (itemIds: number[]) => ({ predictions: itemIds.map((itemId) => prediction(itemId)), thresholds: { general: 0.35, character: 0.35 } }));
    const user = setupUser();
    await renderPanel([1, 2]);
    await screen.findByText('cat');
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));
    expect(mocks.predict).toHaveBeenCalledWith([1, 2], [model.slug]);
    await user.click(screen.getByRole('button', { name: 'Apply 2 tags' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledWith([
      { media_item_id: 1, tags: ['character:cat'] },
      { media_item_id: 2, tags: ['character:cat'] },
    ]));
    expect(mocks.announce).toHaveBeenCalledWith('items.apply_ai_tags');
  });

  it('sends every selected model and media item through one review request', async () => {
    const secondModel = { ...model, slug: 'z3d-e621-convnext', label: 'Z3D' };
    mocks.status.mockResolvedValue({
      models: [model, secondModel],
      configuredModelSlugs: [model.slug, secondModel.slug],
      thresholds: { general: 0.35, character: 0.35 },
      cachedBackend: null,
    });
    mocks.predict.mockImplementation(async (itemIds: number[], modelSlugs: string[]) => ({
      predictions: itemIds.map((mediaItemId) => ({
        mediaItemId,
        error: null,
        predictions: modelSlugs.map((slug) => ({
          tag: slug === model.slug ? 'cat' : 'dog',
          namespace: 'character',
          confidence: 0.8,
          model: slug,
        })),
      })),
      thresholds: { character: 0.35 },
    }));
    await renderPanel([1, 2]);
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledWith(
      [1, 2],
      [model.slug, secondModel.slug],
    ));
    const wdButton = screen.getAllByText('WD14').map((node) => node.closest('button')).find(Boolean);
    expect(wdButton?.className).toContain('sidebarItemSelected');
    const z3dButton = screen.getAllByText('Z3D').map((node) => node.closest('button')).find(Boolean);
    expect(z3dButton?.className).toContain('sidebarItemSelected');
  });

  it('uses namespace thresholds returned by the backend', async () => {
    mocks.predict.mockResolvedValue({ predictions: [prediction(1, 'miku', 0.5)], thresholds: { general: 0.35, character: 0.9 } });
    const user = setupUser();
    await renderPanel();
    await screen.findByText('Below cutoff');
    expect(screen.queryByText('miku')).not.toBeInTheDocument();
    await user.click(screen.getByText('Below cutoff'));
    expect(await screen.findByText('miku')).toBeInTheDocument();
    expect(screen.queryByText(/\d+% cutoff/)).not.toBeInTheDocument();
  });

  it('selects rating predictions above their threshold by default', async () => {
    mocks.predict.mockResolvedValue({
      predictions: [{
        mediaItemId: 1,
        error: null,
        predictions: [{ tag: 'explicit', namespace: 'rating', confidence: 0.9, model: model.slug }],
      }],
      thresholds: { general: 0.35, rating: 0.5 },
    });
    const user = setupUser();
    await renderPanel();
    expect(await screen.findByText('explicit')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledWith([
      { media_item_id: 1, tags: ['rating:explicit'] },
    ]));
  });

  it('uses the inspector preview frame for the reviewed image', async () => {
    const { container } = await renderPanel();
    await screen.findByText('cat');
    const image = container.querySelector('img');
    if (!image) throw new Error('AI review preview image was not rendered');
    expect(image.className).toContain('previewImage');
    expect(image.parentElement?.className).toContain('previewFrame');
  });

  it('reports the execution provider selected when the model loads', async () => {
    const initialStatus = await mocks.status();
    mocks.status
      .mockResolvedValueOnce(initialStatus)
      .mockResolvedValue({ ...initialStatus, cachedBackend: 'CoreML GPU' });
    await renderPanel();
    expect(await screen.findByText(/CoreML GPU inference/)).toBeInTheDocument();
  });

  it('does not apply until the receipt promise resolves and stays open on failure', async () => {
    let resolveApply!: (value: unknown) => void;
    mocks.apply.mockReturnValue(new Promise((resolve) => { resolveApply = resolve; }));
    const user = setupUser();
    await renderPanel();
    await screen.findByText('cat');
    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));
    expect(screen.getByRole('dialog', { name: 'AI tagger' })).toBeInTheDocument();
    await act(async () => { resolveApply({ revision: 1, resources: ['tags'], item_ids: [1] }); });
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'AI tagger' })).not.toBeInTheDocument());
  });

  it('ignores stale prediction completion after the target changes', async () => {
    let resolveFirst!: (value: any) => void;
    let resolveSecond!: (value: any) => void;
    mocks.predict
      .mockReturnValueOnce(new Promise((resolve) => { resolveFirst = resolve; }))
      .mockReturnValueOnce(new Promise((resolve) => { resolveSecond = resolve; }));
    const { store } = await renderPanel([1]);
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));
    act(() => store.set(mocks.targetAtom, { kind: 'explicit', item_ids: [2] }));
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(2));
    await act(async () => {
      resolveFirst({ predictions: [prediction(1, 'stale')], thresholds: { character: 0.35 } });
    });
    expect(screen.queryByText('stale')).not.toBeInTheDocument();
    await act(async () => {
      resolveSecond({ predictions: [prediction(2, 'fresh')], thresholds: { character: 0.35 } });
    });
    expect(await screen.findByText('fresh')).toBeInTheDocument();
  });

  it('reports partial prediction failures without hiding successful tags', async () => {
    mocks.predict.mockImplementation(async (itemIds: number[]) => ({
      predictions: itemIds.map((itemId) => itemId === 1
        ? prediction(1, 'fresh')
        : { mediaItemId: 2, predictions: [], error: 'unsupported media' }),
      thresholds: { character: 0.35 },
    }));
    await renderPanel([1, 2]);
    expect(await screen.findByText('fresh')).toBeInTheDocument();
    expect(await screen.findByText(/1 of 2 media items could not be tagged.*unsupported media/i)).toBeInTheDocument();
  });

  it('reports determinate progress while selected media are analyzed', async () => {
    let resolveFirst!: (value: any) => void;
    mocks.predict.mockReturnValueOnce(new Promise((resolve) => { resolveFirst = resolve; }));
    await renderPanel([1, 2]);
    expect(await screen.findByRole('progressbar')).toHaveAttribute('aria-valuenow', '0');
    expect(screen.getByText('Analyzing 1 of 2')).toBeInTheDocument();
    await act(async () => resolveFirst({ predictions: [prediction(1), prediction(2)], thresholds: { character: 0.35 } }));
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByRole('progressbar')).not.toBeInTheDocument());
  });

  it('reviews per-image predictions with buttons and arrow keys', async () => {
    mocks.predict.mockImplementation(async (itemIds: number[]) => ({
      predictions: itemIds.map((itemId) => prediction(itemId, itemId === 1 ? 'cat' : 'dog')),
      thresholds: { character: 0.35 },
    }));
    await renderPanel([1, 2]);
    expect(await screen.findByText('cat')).toBeInTheDocument();
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));
    fireEvent.keyDown(window, { key: 'ArrowRight' });
    expect(await screen.findByText('dog')).toBeInTheDocument();
    expect(screen.queryByText('cat')).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'ArrowLeft' });
    expect(await screen.findByText('cat')).toBeInTheDocument();
  });

  it('keeps review choices scoped to the image being reviewed', async () => {
    mocks.predict.mockImplementation(async (itemIds: number[]) => ({
      predictions: itemIds.map((itemId) => prediction(itemId)),
      thresholds: { character: 0.35 },
    }));
    const user = setupUser();
    await renderPanel([1, 2]);
    await screen.findByText('cat');
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));
    await user.click(screen.getByText('cat'));
    await user.click(screen.getByRole('button', { name: 'Next image' }));
    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledWith([
      { media_item_id: 2, tags: ['character:cat'] },
    ]));
  });
});
