import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MantineProvider } from '@mantine/core';
import type { ComponentProps } from 'react';
import { PdfViewer } from './PdfViewer';
import type { ViewerZoomControls } from '../../../state/viewer';

const renderPdf = (props: Partial<ComponentProps<typeof PdfViewer>> = {}) => render(
  <MantineProvider><PdfViewer src="media://localhost/file/document.pdf" {...props} /></MantineProvider>,
);

const destroy = vi.fn();
const getViewport = vi.fn(({ scale }: { scale: number }) => ({ width: 600 * scale, height: 800 * scale }));
const getPage = vi.fn().mockResolvedValue({
  getViewport,
  streamTextContent: () => new ReadableStream(),
  render: () => ({ cancel: vi.fn(), promise: Promise.resolve() }),
});

vi.mock('pdfjs-dist/legacy/build/pdf.mjs', () => ({
  GlobalWorkerOptions: { workerSrc: '' },
  getDocument: vi.fn(() => ({
    destroy,
    promise: Promise.resolve({ numPages: 2, getPage }),
  })),
  TextLayer: class {
    render() { return Promise.resolve(); }
    cancel() {}
  },
}));

class MockResizeObserver {
  constructor(private callback: ResizeObserverCallback) {}
  observe() { this.callback([{ contentRect: { width: 900, height: 700 } } as ResizeObserverEntry], this as unknown as ResizeObserver); }
  disconnect() {}
  unobserve() {}
}

describe('PdfViewer', () => {
  let contextSpy: { mockRestore(): void };

  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', MockResizeObserver);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, arrayBuffer: () => Promise.resolve(new ArrayBuffer(8)) }));
    contextSpy = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({} as CanvasRenderingContext2D);
  });

  afterEach(() => {
    contextSpy.mockRestore();
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  it('loads bytes through Picto and exposes page-only controls in the footer', async () => {
    renderPdf();
    await waitFor(() => expect(screen.getByText('Page 1 of 2')).toBeInTheDocument());
    await waitFor(() => expect(getPage).toHaveBeenCalled());
    const next = screen.getByRole('button', { name: 'Next PDF page' });
    fireEvent.click(next);
    expect(screen.getByText('Page 2 of 2')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Previous PDF page' })).toBeEnabled();
    expect(screen.getByRole('contentinfo', { name: 'PDF page navigation' })).toBeInTheDocument();
    expect(screen.queryByRole('slider', { name: 'Zoom' })).not.toBeInTheDocument();
  });

  it('fits against the measured content box without subtracting CSS padding twice', async () => {
    renderPdf();
    await waitFor(() => expect(getViewport).toHaveBeenCalledWith({ scale: 0.875 }));
  });

  it('publishes PDF zoom through the existing viewer-toolbar contract', async () => {
    let controls: ViewerZoomControls | null = null;
    const onZoomControlsChange = vi.fn((value: ViewerZoomControls | null) => { controls = value; });
    const onZoomPercentChange = vi.fn();
    renderPdf({ onZoomControlsChange, onZoomPercentChange });
    await waitFor(() => expect(controls).not.toBeNull());
    await waitFor(() => expect(onZoomPercentChange).toHaveBeenCalled());
    act(() => controls?.setZoomScale(2));
    await waitFor(() => expect(onZoomPercentChange).toHaveBeenCalledWith(200));
    expect(screen.queryByRole('button', { name: 'Fit PDF page' })).not.toBeInTheDocument();
  });

  it('renders a selectable text layer without annotation or editing layers', async () => {
    const { container } = renderPdf();
    await waitFor(() => expect(getPage).toHaveBeenCalled());
    expect(container.querySelector('[class*="textLayer"]')).toBeInTheDocument();
    expect(container.querySelector('.annotationLayer')).not.toBeInTheDocument();
  });
});
