import { fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
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
    const { container } = render(
      <DetailMediaRenderer hash="pdf-hash" mimeType="application/pdf" displayName="Document" />,
    );

    expect(container.querySelector('[data-document-page-preview]')).toBeInTheDocument();
    expect(container.querySelector('[data-document-renderer-snapshot]')).toBeNull();
    expect(await screen.findByTestId('pdf-renderer')).toBeInTheDocument();
  });

  it('uses a generated full-renderer document thumbnail without nesting another shell', async () => {
    const { container } = render(
      <DetailMediaRenderer hash="text-hash" mimeType="text/plain" displayName="Notes" />,
    );

    expect(container.querySelector('[data-document-renderer-snapshot]')).toBeInTheDocument();
    expect(container.querySelector('[data-document-page-preview]')).toBeNull();
    expect(await screen.findByTestId('text-renderer')).toBeInTheDocument();
  });
});
