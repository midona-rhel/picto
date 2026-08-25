import { act, render, screen } from '@testing-library/react';
import { useRef, type ComponentProps } from 'react';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { useMediaImagePipeline, type MediaPipelineInput } from './useMediaImagePipeline';

interface FakeImage {
  src: string;
  onload: (() => void) | null;
  onerror: (() => void) | null;
  decode?: () => Promise<void>;
}

function PipelineHarness(props: Omit<MediaPipelineInput, 'imgRef'>) {
  const imgRef = useRef<HTMLImageElement>(null);
  const pipeline = useMediaImagePipeline({ ...props, imgRef });
  return (
    <output
      data-testid="pipeline"
      data-displayed-hash={pipeline.displayedHash ?? ''}
      data-thumb-url={pipeline.thumbUrl}
      data-full-url={pipeline.fullUrl}
    />
  );
}

const input = (hash: string): ComponentProps<typeof PipelineHarness> => ({
  hash,
  thumbnailHash: `${hash}-thumb`,
  mime: 'image/png',
  isVideo: false,
  neighborHashes: [],
});

describe('useMediaImagePipeline', () => {
  let images: FakeImage[];

  beforeEach(() => {
    vi.useFakeTimers();
    images = [];
    vi.stubGlobal('Image', class {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      private _src = '';

      get src() {
        return this._src;
      }

      set src(value: string) {
        this._src = value;
        images.push(this);
      }
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('starts with the initial thumbnail and delays the full image', () => {
    render(<PipelineHarness {...input('first')} />);

    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-displayed-hash', 'first');
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-thumb-url', 'media://localhost/thumb/first-thumb.jpg');
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-full-url', '');

    act(() => vi.advanceTimersByTime(99));
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-full-url', '');

    act(() => vi.advanceTimersByTime(1));
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-full-url', 'media://localhost/file/first-thumb.png');
  });

  it('swaps on a ready hash change and ignores stale thumbnail loads', () => {
    const { rerender } = render(<PipelineHarness {...input('first')} />);
    const firstPreload = images[0];

    rerender(<PipelineHarness {...input('second')} />);
    const secondPreload = images[1];

    act(() => { firstPreload.onload?.(); });
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-displayed-hash', 'first');

    act(() => { secondPreload.onload?.(); });
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-displayed-hash', 'second');
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-thumb-url', 'media://localhost/thumb/second-thumb.jpg');
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-full-url', '');

    act(() => vi.advanceTimersByTime(100));
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-full-url', 'media://localhost/file/second-thumb.png');
  });

  it('does not swap to a loaded thumbnail until it has decoded', async () => {
    const { rerender } = render(<PipelineHarness {...input('first')} />);
    rerender(<PipelineHarness {...input('second')} />);
    const secondPreload = images[1];
    let finishDecode: (() => void) | undefined;
    secondPreload.decode = vi.fn(() => new Promise<void>((resolve) => { finishDecode = resolve; }));

    act(() => { secondPreload.onload?.(); });
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-displayed-hash', 'first');

    await act(async () => { finishDecode?.(); });
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-displayed-hash', 'second');
  });

  it('reloads a changed thumbnail for the same entity hash', () => {
    const { rerender } = render(<PipelineHarness {...input('first')} />);
    const replacement = { ...input('first'), thumbnailHash: 'first-thumb-v2' };

    rerender(<PipelineHarness {...replacement} />);
    const replacementPreload = images[1];
    expect(replacementPreload).toBeDefined();

    act(() => { replacementPreload.onload?.(); });
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-displayed-hash', 'first');
    expect(screen.getByTestId('pipeline')).toHaveAttribute('data-thumb-url', 'media://localhost/thumb/first-thumb-v2.jpg');
  });
});
