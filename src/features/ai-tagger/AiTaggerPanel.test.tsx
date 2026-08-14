import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Provider, createStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AiTaggerPanel } from './AiTaggerPanel';

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  predict: vi.fn(),
  apply: vi.fn(),
  cancel: vi.fn(),
  portalAtom: undefined as any,
  targetAtom: undefined as any,
}));

vi.mock('../../platform/aiTaggerApi', () => ({
  aiTaggerStatus: mocks.status,
  aiTagPredict: mocks.predict,
  aiTagApply: mocks.apply,
  aiTagCancel: mocks.cancel,
}));

vi.mock('../../runtime/aiTaggerTasks', () => ({
  useAiTaggerTasks: () => ({ autoTag: null, downloads: {} }),
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
  OverlayShell: ({ open, onClose, header, footer, children }: any) => (
    open ? (
      <div role="dialog" aria-label="AI tagger">
        <button type="button" aria-label="Close" onClick={onClose} />
        <div>{header}</div>
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null
  ),
}));

vi.mock('../../shared/ui/ProgressBar', () => ({
  ProgressBar: () => <div role="progressbar" />,
}));

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: any) => children,
}));

const model = {
  slug: 'wd14-swinv2-v3',
  label: 'WD14',
  enabled: true,
  downloaded: true,
  recommended: true,
  heavy: false,
  sizeBytes: 1,
  dataset: 'test',
};

const prediction = (hash: string, tag = 'cat', confidence = 0.8) => ({
  hash,
  error: null,
  tags: [{ tag, namespace: 'character', confidence, model: model.slug }],
});

async function renderPanel(hashes = ['hash-1']) {
  const store = createStore();
  store.set(mocks.portalAtom, { open: true, anchor: null });
  store.set(mocks.targetAtom, { kind: 'entity_hashes', entity_hashes: hashes });
  const result = render(
    <Provider store={store}>
      <AiTaggerPanel />
    </Provider>,
  );
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 60));
  });
  return { ...result, store };
}

async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.status.mockResolvedValue({ models: [model] });
  mocks.predict.mockResolvedValue({
    predictions: [prediction('hash-1')],
    thresholds: { general: 0.35, character: 0.35 },
  });
  mocks.apply.mockResolvedValue(1);
  mocks.cancel.mockResolvedValue(undefined);
});

describe('AiTaggerPanel', () => {
  it('uses namespace thresholds returned by the backend', async () => {
    mocks.predict.mockResolvedValue({
      predictions: [prediction('hash-1', 'miku', 0.5)],
      thresholds: { general: 0.35, character: 0.9 },
    });
    const user = userEvent.setup();
    await renderPanel();

    await waitFor(() => expect(screen.getByText('Below cutoff')).toBeInTheDocument());
    expect(screen.queryByText('miku')).not.toBeInTheDocument();
    await user.click(screen.getByText('Below cutoff'));
    expect(await screen.findByText('miku')).toBeInTheDocument();
  });

  it('sends one atomic per-image assignment call', async () => {
    mocks.predict.mockResolvedValue({
      predictions: [prediction('hash-1'), prediction('hash-2')],
      thresholds: { general: 0.35, character: 0.35 },
    });
    const user = userEvent.setup();
    await renderPanel(['hash-1', 'hash-2']);

    await screen.findByText('cat');
    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));

    await waitFor(() => expect(mocks.apply).toHaveBeenCalledTimes(1));
    expect(mocks.apply).toHaveBeenCalledWith([
      { hash: 'hash-1', tags: ['character:cat'] },
      { hash: 'hash-2', tags: ['character:cat'] },
    ]);
  });

  it('waits for apply before closing and stays open when apply fails', async () => {
    let resolveApply!: (value: number) => void;
    mocks.apply.mockReturnValue(new Promise<number>((resolve) => { resolveApply = resolve; }));
    const user = userEvent.setup();
    await renderPanel();
    await screen.findByText('cat');

    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));
    expect(screen.getByRole('dialog', { name: 'AI tagger' })).toBeInTheDocument();
    await act(async () => {
      resolveApply(1);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'AI tagger' })).not.toBeInTheDocument());

    mocks.apply.mockRejectedValueOnce(new Error('apply failed'));
    const second = await renderPanel();
    await screen.findByText('cat');
    await user.click(screen.getByRole('button', { name: 'Apply 1 tag' }));
    await waitFor(() => expect(mocks.apply).toHaveBeenCalledTimes(2));
    expect(screen.getByRole('dialog', { name: 'AI tagger' })).toBeInTheDocument();
    second.unmount();
  });

  it('ignores stale prediction completion after the panel closes', async () => {
    let resolvePredict!: (value: any) => void;
    mocks.predict.mockReturnValue(new Promise((resolve) => { resolvePredict = resolve; }));
    const user = userEvent.setup();
    await renderPanel();
    await waitFor(() => expect(mocks.predict).toHaveBeenCalled());

    await user.click(screen.getByRole('button', { name: 'Close' }));
    resolvePredict({ predictions: [prediction('hash-1', 'stale')], thresholds: { character: 0.35 } });
    await settle();

    expect(screen.queryByText('stale')).not.toBeInTheDocument();
    expect(mocks.cancel).toHaveBeenCalledTimes(1);
  });

  it('ignores stale prediction completion after the target changes', async () => {
    let resolveFirst!: (value: any) => void;
    let resolveSecond!: (value: any) => void;
    mocks.predict
      .mockReturnValueOnce(new Promise((resolve) => { resolveFirst = resolve; }))
      .mockReturnValueOnce(new Promise((resolve) => { resolveSecond = resolve; }));
    const { store } = await renderPanel(['hash-1']);
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(1));

    act(() => {
      store.set(mocks.targetAtom, { kind: 'entity_hashes', entity_hashes: ['hash-2'] });
    });
    await waitFor(() => expect(mocks.predict).toHaveBeenCalledTimes(2));

    resolveFirst({ predictions: [prediction('hash-1', 'stale')], thresholds: { character: 0.35 } });
    await settle();
    expect(screen.queryByText('stale')).not.toBeInTheDocument();

    resolveSecond({ predictions: [prediction('hash-2', 'fresh')], thresholds: { character: 0.35 } });
    expect(await screen.findByText('fresh')).toBeInTheDocument();
  });

  it('discloses partial prediction failures without hiding successful tags', async () => {
    mocks.predict.mockResolvedValue({
      predictions: [
        prediction('hash-1', 'fresh'),
        { hash: 'hash-2', tags: [], error: 'unsupported media' },
      ],
      thresholds: { character: 0.35 },
    });

    await renderPanel(['hash-1', 'hash-2']);

    expect(await screen.findByText('fresh')).toBeInTheDocument();
    expect(screen.getByText(/1 of 2 images could not be tagged.*unsupported media/i)).toBeInTheDocument();
  });
});
