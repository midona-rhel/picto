import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Provider, createStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AiTaggerPanel } from './AiTaggerPanel';

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  predict: vi.fn(),
  apply: vi.fn(),
  portalAtom: undefined as any,
  targetAtom: undefined as any,
}));

vi.mock('../../platform/aiTaggerApi', () => ({
  aiTaggerStatus: mocks.status,
  aiTagPredict: mocks.predict,
  aiTagApply: mocks.apply,
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

async function renderPanel(itemIds = [1]) {
  const store = createStore();
  store.set(mocks.portalAtom, { open: true, anchor: null });
  store.set(mocks.targetAtom, { kind: 'explicit', item_ids: itemIds });
  const result = render(<Provider store={store}><AiTaggerPanel /></Provider>);
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  return { ...result, store };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.status.mockResolvedValue({
    models: [model], configuredModelSlugs: [model.slug],
    thresholds: { general: 0.35, character: 0.35, copyright: 0.35, artist: 0.35, species: 0.35, rating: 0.35 },
    cachedBackend: null,
  });
  mocks.predict.mockResolvedValue({ predictions: [prediction(1)], thresholds: { general: 0.35, character: 0.35 } });
  mocks.apply.mockResolvedValue({ revision: 1, resources: ['tags'], item_ids: [1] });
});

describe('AiTaggerPanel', () => {
  it('uses replacement numeric item IDs for prediction and apply', async () => {
    mocks.predict.mockResolvedValue({ predictions: [prediction(1), prediction(2)], thresholds: { general: 0.35, character: 0.35 } });
    const user = userEvent.setup();
    await renderPanel([1, 2]);
    await screen.findByText('cat');
    expect(mocks.predict).toHaveBeenCalledWith([1, 2], [model.slug]);
    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledWith([
      { media_item_id: 1, tags: ['character:cat'] },
      { media_item_id: 2, tags: ['character:cat'] },
    ]));
  });

  it('uses namespace thresholds returned by the backend', async () => {
    mocks.predict.mockResolvedValue({ predictions: [prediction(1, 'miku', 0.5)], thresholds: { general: 0.35, character: 0.9 } });
    const user = userEvent.setup();
    await renderPanel();
    await screen.findByText('Below cutoff');
    expect(screen.queryByText('miku')).not.toBeInTheDocument();
    await user.click(screen.getByText('Below cutoff'));
    expect(await screen.findByText('miku')).toBeInTheDocument();
  });

  it('does not apply until the receipt promise resolves and stays open on failure', async () => {
    let resolveApply!: (value: unknown) => void;
    mocks.apply.mockReturnValue(new Promise((resolve) => { resolveApply = resolve; }));
    const user = userEvent.setup();
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
    resolveFirst({ predictions: [prediction(1, 'stale')], thresholds: { character: 0.35 } });
    await act(async () => {});
    expect(screen.queryByText('stale')).not.toBeInTheDocument();
    resolveSecond({ predictions: [prediction(2, 'fresh')], thresholds: { character: 0.35 } });
    expect(await screen.findByText('fresh')).toBeInTheDocument();
  });

  it('reports partial prediction failures without hiding successful tags', async () => {
    mocks.predict.mockResolvedValue({ predictions: [prediction(1, 'fresh'), { mediaItemId: 2, predictions: [], error: 'unsupported media' }], thresholds: { character: 0.35 } });
    await renderPanel([1, 2]);
    expect(await screen.findByText('fresh')).toBeInTheDocument();
    expect(screen.getByText(/1 of 2 media items could not be tagged.*unsupported media/i)).toBeInTheDocument();
  });
});
