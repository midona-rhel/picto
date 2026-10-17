import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import { GridQuickLook } from './GridQuickLook';

vi.mock('../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

vi.mock('../groups/GroupQuickLook', () => ({
  GroupQuickLookContent: () => <div data-testid="group-content" />,
}));

const media = (root_id: number, content_hash: string, width: number, height: number): CanonicalEntityGridItem => ({
  root_id, content_hash, width, height,
  kind: 'media', lifecycle: 'inbox', name: content_hash, mime: 'image/jpeg',
  cover_media_id: root_id, duration_ms: null, frame_count: null, palette: [],
  imported_at_ms: 0, captured_at_ms: null, modified_at_ms: 0,
  media_count: 1, total_size_bytes: 0, rating: 'unrated',
});
const portrait = media(8, 'portrait', 800, 1600);
const landscape = media(9, 'landscape', 1600, 800);
const group: CanonicalEntityGridItem = {
  ...media(7, 'group', 800, 800), kind: 'collection', mime: 'application/x-collection',
};
const callbacks = { onNavigate: vi.fn(), onClose: vi.fn(), recordItemId: null };

let pendingDecodes: Array<{ url: string; resolve: () => void }>;

// Only browser loading/layout are controlled. Content, pipeline, frame and zoom are real.
beforeEach(() => {
  vi.useFakeTimers();
  pendingDecodes = [];
  vi.stubGlobal('Image', class {
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    private url = '';
    decode = () => new Promise<void>((resolve) => {
      pendingDecodes.push({ url: this.url, resolve });
    });
    set src(value: string) { this.url = value; this.onload?.(); }
  });
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    disconnect() {}
  });
  // jsdom has no layout; give the real fit-to-window code an 800 x 600 viewport.
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(800);
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(600);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function decodeThumbnail(hash: string) {
  const url = `media://localhost/thumb/${hash}.jpg`;
  const ready = pendingDecodes.filter((decode) => decode.url === url);
  pendingDecodes = pendingDecodes.filter((decode) => decode.url !== url);
  await act(async () => { ready.forEach((decode) => decode.resolve()); });
}

function expectImage(hash: string, width: number, height: number, scale: number) {
  const frame = document.body.querySelector('[data-image-crossfade-frame]');
  expect(frame).toHaveStyle({
    width: `${width}px`, height: `${height}px`, aspectRatio: `${width} / ${height}`,
    transform: `translate(calc(-50% + 0px), calc(-50% + 0px)) scale(${scale})`,
  });
  expect(frame?.querySelector('[data-image-crossfade-layer="thumbnail"]')).toHaveAttribute(
    'src', `media://localhost/thumb/${hash}.jpg`,
  );
  expect(frame?.querySelector('[data-progressive-media-preview]')).toHaveAttribute('data-visible', 'true');
  expect(document.body.querySelector('[data-quick-look-overlay]')).toHaveAttribute('data-media-ready', 'true');
  return frame!;
}

describe('GridQuickLook', () => {
  it.each([
    { index: 1, windowStart: 0, totalCount: undefined, previous: true, next: false },
    { index: 0, windowStart: 0, totalCount: undefined, previous: false, next: true },
    { index: 1, windowStart: 20, totalCount: 30, previous: true, next: true },
    { index: 1, windowStart: 20, totalCount: 22, previous: true, next: false },
  ])('preserves keyboard bounds while a removed image is retained at $windowStart + $index', async ({
    index, windowStart, totalCount, previous, next,
  }) => {
    const before = media(10, 'before', 1600, 800);
    const items = index === 0 ? [portrait, before, landscape] : [before, portrait, landscape];
    const onNavigate = vi.fn();
    const props = { ...callbacks, onNavigate, windowStart, totalCount, currentIndex: index };
    const { rerender } = render(<GridQuickLook {...props} items={items} />);
    await decodeThumbnail('portrait');
    rerender(<GridQuickLook {...props} items={items.filter((item) => item !== portrait)} />);
    expectImage('portrait', 800, 1600, 0.375);

    fireEvent.keyDown(window, { key: 'ArrowLeft' });
    expect(onNavigate.mock.calls).toEqual(previous ? [[-1]] : []);
    onNavigate.mockClear();
    fireEvent.keyDown(window, { key: 'ArrowRight' });
    expect(onNavigate.mock.calls).toEqual(next ? [[1]] : []);
    expectImage('portrait', 800, 1600, 0.375);
  });

  it('keeps the decoded group visible until the adjacent media thumbnail is decoded', async () => {
    const items = [group, portrait];
    const { rerender } = render(<GridQuickLook {...callbacks} items={items} currentIndex={0} />);
    const overlay = document.body.querySelector('[data-quick-look-overlay]');

    expect(overlay).toHaveAttribute('data-media-ready', 'true');
    expect(screen.getByTestId('group-content')).toBeInTheDocument();
    rerender(<GridQuickLook {...callbacks} items={items} currentIndex={1} />);
    expect(document.body.querySelector('[data-quick-look-overlay]')).toBe(overlay);
    expect(screen.getByTestId('group-content')).toBeInTheDocument();

    await decodeThumbnail('portrait');
    expect(screen.queryByTestId('group-content')).not.toBeInTheDocument();
    expectImage('portrait', 800, 1600, 0.375);
  });

  it('reports the item on screen only once its image is decoded', async () => {
    const items = [portrait, landscape];
    const onDisplayedItemChange = vi.fn();
    const props = { ...callbacks, items, onDisplayedItemChange };
    const { rerender } = render(<GridQuickLook {...props} currentIndex={0} />);
    expect(onDisplayedItemChange).toHaveBeenLastCalledWith(portrait.root_id);

    rerender(<GridQuickLook {...props} currentIndex={1} />);
    expect(onDisplayedItemChange).toHaveBeenLastCalledWith(portrait.root_id);
    await decodeThumbnail('landscape');
    await decodeThumbnail('landscape');
    expect(onDisplayedItemChange).toHaveBeenLastCalledWith(landscape.root_id);
  });

  it('keeps the previous image at its own size until the next image thumbnail is decoded', async () => {
    const items = [portrait, landscape];
    const { rerender } = render(<GridQuickLook {...callbacks} items={items} currentIndex={0} />);
    await decodeThumbnail('portrait');
    expectImage('portrait', 800, 1600, 0.375);

    rerender(<GridQuickLook {...callbacks} items={items} currentIndex={1} />);
    expectImage('portrait', 800, 1600, 0.375);
    await decodeThumbnail('landscape'); // Grid handoff; content is still decoding.
    expectImage('portrait', 800, 1600, 0.375);
    await decodeThumbnail('landscape');
    expectImage('landscape', 1600, 800, 0.5);
  });

  it.each([
    { accepted: portrait, next: landscape, width: 800, height: 1600, scale: 0.375, nextWidth: 1600, nextHeight: 800, nextScale: 0.5 },
    { accepted: landscape, next: portrait, width: 1600, height: 800, scale: 0.5, nextWidth: 800, nextHeight: 1600, nextScale: 0.375 },
  ])('retains the accepted $accepted.content_hash bitmap and fitted size after it leaves the inbox', async ({
    accepted, next, width, height, scale, nextWidth, nextHeight, nextScale,
  }) => {
    const items = [group, accepted, next];
    const { rerender } = render(<GridQuickLook {...callbacks} items={items} currentIndex={0} />);

    // Navigate first so the accepted image has an already-decoded grid preview.
    rerender(<GridQuickLook {...callbacks} items={items} currentIndex={1} />);
    await decodeThumbnail(accepted.content_hash);
    await decodeThumbnail(accepted.content_hash);
    const frame = expectImage(accepted.content_hash, width, height, scale);
    act(() => vi.advanceTimersByTime(100));
    const fullImage = frame.querySelector('[data-image-crossfade-layer="full"]')!;
    fireEvent.load(fullImage);
    expect(fullImage).toHaveAttribute('src', `media://localhost/file/${accepted.content_hash}.jpg`);
    expect(frame.querySelector('[data-progressive-media-content]')).toHaveAttribute('data-visible', 'true');

    // Accepting removes the item; the same index now points to the opposite aspect ratio.
    rerender(<GridQuickLook {...callbacks} items={[group, next]} currentIndex={1} />);
    expectImage(accepted.content_hash, width, height, scale);

    // Cached old-preview decodes may finish before the replacement. Reusing that
    // bitmap with the next item's metadata must not stretch it or refit the frame.
    await decodeThumbnail(accepted.content_hash);
    expectImage(accepted.content_hash, width, height, scale);

    await decodeThumbnail(next.content_hash); // Grid hands off to the real content pipeline.
    expectImage(accepted.content_hash, width, height, scale);
    await decodeThumbnail(next.content_hash);
    expectImage(next.content_hash, nextWidth, nextHeight, nextScale);
  });
});
