import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Provider, createStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderWithProviders } from '../../test/render';
import { AiTaggerPanel } from './AiTaggerPanel';

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  predict: vi.fn(),
  unload: vi.fn(),
  apply: vi.fn(),
  announce: vi.fn(),
  details: vi.fn(),
  portalAtom: undefined as any,
  targetAtom: undefined as any,
}));

vi.mock('../../platform/aiTaggerApi', () => ({
  aiTaggerStatus: mocks.status,
  aiTagPredict: mocks.predict,
  aiTaggerUnload: mocks.unload,
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

const prediction = (rootId: number, tag = 'cat', confidence = 0.8) => ({
  rootId,
  error: null,
  predictions: [{ tag, namespace: 'character', confidence, model: model.slug }],
});

const details = (itemId: number) => ({
  root: {
    root_id: itemId,
    stable_key: `root-${itemId}`,
    kind: 'media',
    name: `Image ${itemId}`,
    notes: null,
    source_urls: [],
    cover_media_id: itemId,
    imported_at_ms: Date.parse('2026-01-01T00:00:00Z'),
    captured_at_ms: null,
    modified_at_ms: Date.parse('2026-01-01T00:00:00Z'),
    media_count: 1,
    total_size_bytes: 100,
  },
  lifecycle: 'active',
  rating: 'unrated',
  folder_ids: [],
  tag_ids: [],
  revision: 1,
  media: [{
    media_id: itemId,
    media_name: `Image ${itemId}`,
    file_id: itemId,
    file_path: `/media/hash-${itemId}`,
    facts: {
      mime: 'image/jpeg',
      size_bytes: 100,
      width: 100,
      height: 100,
      duration_ms: null,
      frame_count: null,
      content_hash: `hash-${itemId}`,
      perceptual_hash: null,
      palette: [],
    },
  }],
});

const collectionDetails = (itemId: number, mediaItemIds: number[]) => ({
  ...details(itemId),
  root: {
    ...details(itemId).root,
    kind: 'collection',
    name: 'Mixed collection',
    cover_media_id: mediaItemIds[0],
    media_count: mediaItemIds.length,
    total_size_bytes: mediaItemIds.length * 100,
  },
  media: mediaItemIds.map((mediaItemId) => ({
    ...details(mediaItemId).media[0],
    media_id: mediaItemId,
    media_name: `Image ${mediaItemId}`,
  })),
});

async function renderPanel(itemIds = [1]) {
  const store = createStore();
  store.set(mocks.portalAtom, { open: true, anchor: null });
  store.set(mocks.targetAtom, { kind: 'explicit', root_ids: itemIds });
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

async function startRun() {
  const button = await screen.findByRole('button', { name: 'Run' });
  await waitFor(() => expect(button).toBeEnabled());
  await setupUser().click(button);
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.status.mockResolvedValue({
    models: [model], configuredModelSlugs: [model.slug],
    thresholds: { general: 0.35, character: 0.35, copyright: 0.35, artist: 0.35, species: 0.35, rating: 0.35 },
    cachedBackend: null,
  });
  mocks.predict.mockResolvedValue({ predictions: [prediction(1)], thresholds: { general: 0.35, character: 0.35 } });
  mocks.unload.mockResolvedValue(undefined);
  mocks.details.mockImplementation(async (itemId: number) => details(itemId));
  mocks.apply.mockResolvedValue({ revision: 1, resources: ['tags'], item_ids: [1] });
  mocks.announce.mockResolvedValue(undefined);
});

describe('AiTaggerPanel', () => {
  it('keeps the sidebar mounted while collapsing it', async () => {
    const user = setupUser();
    await renderPanel();
    expect(mocks.predict).not.toHaveBeenCalled();
    const sidebar = screen.getByRole('button', { name: /^Suggested\b/ }).parentElement;
    expect(sidebar?.className).not.toContain('sidebarHidden');

    await user.click(screen.getByRole('button', { name: 'Hide sidebar' }));

    expect(screen.getByRole('button', { name: /^Suggested\b/ }).parentElement).toBe(sidebar);
    expect(sidebar?.className).toContain('sidebarHidden');
  });

  it('unloads the active model when the review portal closes', async () => {
    await renderPanel();
    await setupUser().click(screen.getByRole('button', { name: 'Close' }));
    expect(mocks.unload).toHaveBeenCalled();
  });

  it('uses replacement numeric item IDs for prediction and apply', async () => {
    mocks.predict.mockImplementation(async (itemIds: number[]) => ({ predictions: itemIds.map((itemId) => prediction(itemId)), thresholds: { general: 0.35, character: 0.35 } }));
    const user = setupUser();
    await renderPanel([1, 2]);
    await startRun();
    await screen.findByText('cat');
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));
    expect(mocks.predict).toHaveBeenCalledWith([1, 2], [model.slug]);
    await user.click(screen.getByRole('button', { name: 'Apply 2 tags' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledWith([
      { root_id: 1, tags: ['character:cat'] },
      { root_id: 2, tags: ['character:cat'] },
    ]));
    expect(mocks.announce).toHaveBeenCalledWith('items.apply_ai_tags');
  });

  it('runs all selected roots once through each selected model', async () => {
    const secondModel = { ...model, slug: 'z3d-e621-convnext', label: 'Z3D' };
    mocks.status.mockResolvedValue({
      models: [model, secondModel],
      configuredModelSlugs: [model.slug, secondModel.slug],
      thresholds: { general: 0.35, character: 0.35 },
      cachedBackend: null,
    });
    mocks.predict.mockImplementation(async (itemIds: number[], modelSlugs: string[]) => ({
      predictions: itemIds.map((rootId) => ({
        rootId,
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
    const z3dButton = screen.getAllByText('Z3D').map((node) => node.closest('button')).find(Boolean)!;
    await startRun();
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(2));
    expect(mocks.predict.mock.calls).toEqual([
      [[1, 2], [model.slug]],
      [[1, 2], [secondModel.slug]],
    ]);
    const wdButton = screen.getAllByText('WD14').map((node) => node.closest('button')).find(Boolean);
    expect(wdButton?.className).toContain('sidebarItemSelected');
    expect(z3dButton?.className).toContain('sidebarItemSelected');
  });

  it('reviews a collection once, unions member predictions, and applies once to its root', async () => {
    mocks.details.mockResolvedValue(collectionDetails(10, [11, 12]));
    mocks.predict.mockImplementation(async (itemIds: number[]) => ({
      predictions: itemIds.map((itemId) => ({
        ...prediction(itemId, 'cat'),
        predictions: [
          ...prediction(itemId, 'cat').predictions,
          ...prediction(itemId, 'dog').predictions,
        ],
      })),
      thresholds: { character: 0.35 },
    }));
    const user = setupUser();
    await renderPanel([10]);
    await startRun();

    expect(await screen.findByText('cat')).toBeInTheDocument();
    expect(await screen.findByText('dog')).toBeInTheDocument();
    expect(mocks.predict).toHaveBeenCalledTimes(1);
    expect(mocks.predict).toHaveBeenCalledWith([10], [model.slug]);
    expect(screen.getByText('Mixed collection')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Apply 2 tags' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledWith([
      { root_id: 10, tags: ['character:cat', 'character:dog'] },
    ]));
  });

  it('starts manual review at the general confidence setting', async () => {
    mocks.predict.mockResolvedValue({ predictions: [prediction(1, 'miku', 0.3)], thresholds: { general: 0.35, character: 0.9 } });
    const user = setupUser();
    await renderPanel();
    await startRun();
    await screen.findByText('Below cutoff');
    expect(screen.queryByText('miku')).not.toBeInTheDocument();
    await user.click(screen.getByText('Below cutoff'));
    expect(await screen.findByText('miku')).toBeInTheDocument();
    expect(screen.queryByText(/\d+% cutoff/)).not.toBeInTheDocument();
  });

  it('uses the same run confidence for Picto namespaces', async () => {
    mocks.predict.mockResolvedValue({
      predictions: [{
        rootId: 1,
        error: null,
        predictions: [{ tag: 'example', namespace: 'creator', confidence: 0.3, model: model.slug }],
      }],
      thresholds: { general: 0.35, artist: 0.9 },
    });
    const user = setupUser();
    await renderPanel();
    await startRun();
    await screen.findByText('Below cutoff');
    expect(screen.queryByText('example')).not.toBeInTheDocument();
    await user.click(screen.getByText('Below cutoff'));
    expect(await screen.findByText('example')).toBeInTheDocument();
  });

  it('selects rating predictions above their threshold by default', async () => {
    mocks.predict.mockResolvedValue({
      predictions: [{
        rootId: 1,
        error: null,
        predictions: [{ tag: 'explicit', namespace: 'rating', confidence: 0.9, model: model.slug }],
      }],
      thresholds: { general: 0.35, rating: 0.5 },
    });
    const user = setupUser();
    await renderPanel();
    await startRun();
    expect(await screen.findByText('explicit')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledWith([
      { root_id: 1, tags: ['rating:explicit'] },
    ]));
  });

  it('reclassifies and selects retained predictions when run confidence is released', async () => {
    mocks.predict.mockResolvedValue({ predictions: [prediction(1, 'miku', 0.3)], thresholds: { general: 0.35, character: 0.9 } });
    const user = setupUser();
    await renderPanel();
    await startRun();

    await user.click(screen.getByText('Below cutoff'));
    expect(await screen.findByText('miku')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Apply 1 tag' })).not.toBeInTheDocument();

    const slider = screen.getByRole('slider', { name: 'Run confidence' });
    fireEvent.change(slider, { target: { value: '25' } });
    expect(screen.getByText('25%')).toBeInTheDocument();
    expect(screen.getByText('miku')).toBeInTheDocument();

    fireEvent.pointerUp(slider);
    expect(screen.queryByText('miku')).not.toBeInTheDocument();
    await user.click(screen.getByText('Suggested'));
    expect(await screen.findByText('miku')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Apply 1 tag' })).toBeInTheDocument();
  });

  it('preserves an explicit tag choice while confidence changes', async () => {
    mocks.predict.mockResolvedValue({ predictions: [prediction(1, 'miku', 0.3)], thresholds: { general: 0.35, character: 0.9 } });
    const user = setupUser();
    await renderPanel();
    await startRun();
    await user.click(screen.getByText('Below cutoff'));
    await user.click(await screen.findByText('miku'));

    const slider = screen.getByRole('slider', { name: 'Run confidence' });
    fireEvent.change(slider, { target: { value: '25' } });
    fireEvent.pointerUp(slider);
    await user.click(screen.getByText('Suggested'));

    expect(screen.getByRole('button', { name: 'Apply 1 tag' })).toBeInTheDocument();
  });

  it('uses the inspector preview frame for the reviewed image', async () => {
    const { container } = await renderPanel();
    await startRun();
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
    await startRun();
    expect(await screen.findByText(/CoreML GPU inference/)).toBeInTheDocument();
  });

  it('does not apply until the receipt promise resolves and stays open on failure', async () => {
    let resolveApply!: (value: unknown) => void;
    mocks.apply.mockReturnValue(new Promise((resolve) => { resolveApply = resolve; }));
    const user = setupUser();
    await renderPanel();
    await startRun();
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
    await startRun();
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));
    act(() => store.set(mocks.targetAtom, { kind: 'explicit', root_ids: [2] }));
    await act(async () => {
      resolveFirst({ predictions: [prediction(1, 'stale')], thresholds: { character: 0.35 } });
    });
    expect(screen.queryByText('stale')).not.toBeInTheDocument();
    await startRun();
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(2));
    await act(async () => {
      resolveSecond({ predictions: [prediction(2, 'fresh')], thresholds: { character: 0.35 } });
    });
    expect(await screen.findByText('fresh')).toBeInTheDocument();
  });

  it('reports partial prediction failures without hiding successful tags', async () => {
    mocks.predict.mockImplementation(async (itemIds: number[]) => ({
      predictions: itemIds.map((itemId) => itemId === 1
        ? prediction(1, 'fresh')
        : { rootId: 2, predictions: [], error: 'unsupported media' }),
      thresholds: { character: 0.35 },
    }));
    await renderPanel([1, 2]);
    await startRun();
    expect(await screen.findByText('fresh')).toBeInTheDocument();
    expect(await screen.findByText(/1 of 2 model\/item analyses failed.*unsupported media/i)).toBeInTheDocument();
  });

  it('reports determinate progress while selected media are analyzed', async () => {
    let resolveFirst!: (value: any) => void;
    mocks.predict.mockReturnValueOnce(new Promise((resolve) => { resolveFirst = resolve; }));
    await renderPanel([1, 2]);
    await startRun();
    expect(await screen.findByRole('progressbar')).toHaveAttribute('aria-valuenow', '0');
    expect(screen.getByText('Analyzing 1 of 2')).toBeInTheDocument();
    await act(async () => resolveFirst({ predictions: [prediction(1), prediction(2)], thresholds: { character: 0.35 } }));
    await waitFor(() => expect(screen.queryByRole('progressbar')).not.toBeInTheDocument());
  });

  it('reviews per-image predictions with buttons and arrow keys', async () => {
    mocks.predict.mockImplementation(async (itemIds: number[]) => ({
      predictions: itemIds.map((itemId) => prediction(itemId, itemId === 1 ? 'cat' : 'dog')),
      thresholds: { character: 0.35 },
    }));
    await renderPanel([1, 2]);
    await startRun();
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
    await startRun();
    await screen.findByText('cat');
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));
    await user.click(screen.getByText('cat'));
    await user.click(screen.getByRole('button', { name: 'Next item' }));
    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledWith([
      { root_id: 2, tags: ['character:cat'] },
    ]));
  });
});
