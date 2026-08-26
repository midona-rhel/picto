import { fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { DetailMediaRenderer } from './DetailMediaRenderer';

vi.mock('../../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children }: { children: ReactNode }) => children,
}));

vi.mock('../video/VideoPlayer', () => ({
  VideoPlayer: ({ onReady }: { onReady?: () => void }) => (
    <button type="button" data-testid="audio-renderer" onClick={onReady}>Audio</button>
  ),
}));

vi.mock('./PdfViewer', () => ({
  PdfViewer: () => <div data-testid="pdf-renderer" />,
}));

vi.mock('./TextDocumentViewer', () => ({
  TextDocumentViewer: () => <div data-testid="text-renderer" />,
}));

describe('DetailMediaRenderer', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('keeps a thumbnail visible until a non-image renderer reports usable content', () => {
    const { container } = render(
      <DetailMediaRenderer hash="audio-hash" mimeType="audio/mpeg" displayName="Track" />,
    );

    expect(container.querySelector('[data-progressive-media-renderer]')).toHaveAttribute('data-ready', 'false');
    expect(container.querySelector('img')).toHaveAttribute('src', 'media://localhost/thumb/audio-hash.jpg');

    fireEvent.click(screen.getByTestId('audio-renderer'));

    expect(container.querySelector('[data-progressive-media-renderer]')).toHaveAttribute('data-ready', 'true');
    expect(container.querySelector('img')).toHaveAttribute('src', 'media://localhost/thumb/audio-hash.jpg');
    expect(container.querySelector('[data-progressive-media-preview]')).toHaveAttribute('data-visible', 'false');
    expect(container.querySelector('[data-progressive-media-content]')).toHaveAttribute('data-visible', 'true');
  });

  it('shows the truthful unsupported-type surface without a fake loading phase', () => {
    const { container } = render(
      <DetailMediaRenderer hash="binary-hash" mimeType="application/octet-stream" displayName="Data" />,
    );

    expect(container.querySelector('[data-progressive-media-renderer]')).toBeNull();
    expect(screen.getByRole('alert')).toHaveTextContent('Preview is not available');
  });

  it('places a page-only PDF thumbnail inside the document viewport shell', async () => {
    class TestResizeObserver {
      constructor(private readonly callback: ResizeObserverCallback) {}
      observe() {
        this.callback([{ contentRect: { width: 900, height: 700 } } as ResizeObserverEntry], this as unknown as ResizeObserver);
      }
      disconnect() {}
      unobserve() {}
    }
    vi.stubGlobal('ResizeObserver', TestResizeObserver);
    const { container } = render(
      <DetailMediaRenderer hash="pdf-hash" mimeType="application/pdf" displayName="Document" />,
    );

    const thumbnail = container.querySelector('[data-document-page-preview] img') as HTMLImageElement;
    expect(thumbnail).toHaveStyle({ visibility: 'hidden' });
    Object.defineProperty(thumbnail, 'naturalWidth', { configurable: true, value: 600 });
    Object.defineProperty(thumbnail, 'naturalHeight', { configurable: true, value: 800 });
    fireEvent.load(thumbnail);

    // The final PDF renderer is height-bound in this viewport: it leaves 187.5px
    // on either side instead of stretching the thumbnail to the available width.
    expect(thumbnail).toHaveStyle({ width: '525px', height: '700px', visibility: 'visible' });
    expect(container.querySelector('[data-document-page-preview]')).toBeInTheDocument();
    expect(container.querySelector('[data-document-renderer-snapshot]')).toBeNull();
    expect(await screen.findByTestId('pdf-renderer')).toBeInTheDocument();
  });

  it('uses the eventual full video frame for its thumbnail preview', () => {
    const { container } = render(
      <DetailMediaRenderer hash="video-hash" mimeType="video/mp4" displayName="Movie" />,
    );

    expect(container.querySelector('[data-video-frame-preview]')).toBeInTheDocument();
    expect(container.querySelector('[data-document-page-preview]')).toBeNull();
  });

  it('uses a generated full-renderer document thumbnail without nesting another shell', async () => {
    const { container } = render(
      <DetailMediaRenderer hash="text-hash" mimeType="text/plain" displayName="Notes" />,
    );

    expect(container.querySelector('[data-document-renderer-snapshot]')).toBeInTheDocument();
    expect(container.querySelector('[data-document-renderer-snapshot]')).toHaveAttribute('data-document-kind', 'text-document');
    expect(container.querySelector('[data-document-renderer="true"]')).toBeInTheDocument();
    expect(container.querySelector('[data-document-page-preview]')).toBe(container.querySelector('[data-document-renderer-snapshot]'));
    expect(await screen.findByTestId('text-renderer')).toBeInTheDocument();
  });
});
