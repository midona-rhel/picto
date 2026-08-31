import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { GlobalWorkerOptions, getDocument, TextLayer, type PDFDocumentLoadingTask, type PDFDocumentProxy } from 'pdfjs-dist/legacy/build/pdf.mjs';
import workerUrl from 'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url';
import type { ViewerZoomControls } from '../../../state/viewer';
import { DocumentViewerShell } from './DocumentViewerShell';
import { documentCanvasGeometry, fitDocumentPage } from './documentPageGeometry';
import styles from './PdfViewer.module.css';
import { t } from '../../../i18n';

GlobalWorkerOptions.workerSrc = workerUrl;

interface PdfViewerProps {
  src: string;
  onReady?: () => void;
  onZoomControlsChange?: (controls: ViewerZoomControls | null) => void;
  onZoomPercentChange?: (percent: number) => void;
}

export function PdfViewer({ src, onReady, onZoomControlsChange, onZoomPercentChange }: PdfViewerProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const textLayerRef = useRef<HTMLDivElement>(null);
  const [document, setDocument] = useState<PDFDocumentProxy | null>(null);
  const [pageNumber, setPageNumber] = useState(1);
  const [scaleMode, setScaleMode] = useState<'page-fit' | 'page-width' | 'actual' | 'custom'>('page-fit');
  const [customScale, setCustomScale] = useState(1);
  const [renderedScale, setRenderedScale] = useState(1);
  const renderedScaleRef = useRef(1);
  const zoomListenersRef = useRef(new Set<(scale: number) => void>());
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const abort = new AbortController();
    let task: PDFDocumentLoadingTask | null = null;
    setDocument(null);
    setPageNumber(1);
    setScaleMode('page-fit');
    setCustomScale(1);
    setError(null);
    void fetch(src, { signal: abort.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`PDF request failed (${response.status})`);
        return response.arrayBuffer();
      })
      .then((buffer) => {
        task = getDocument({ data: new Uint8Array(buffer) });
        return task.promise;
      })
      .then(setDocument)
      .catch((reason: unknown) => {
        if (!abort.signal.aborted) {
          setError(reason instanceof Error ? reason.message : t('Could not open this PDF.'));
          onReady?.();
        }
      });
    return () => {
      abort.abort();
      void task?.destroy();
    };
  }, [onReady, src]);

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      setSize({ width: entry.contentRect.width, height: entry.contentRect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!document || !canvas || size.width === 0 || size.height === 0) return;
    let cancelled = false;
    let renderTask: { cancel(): void; promise: Promise<unknown> } | null = null;
    let textLayer: TextLayer | null = null;

    void document.getPage(pageNumber).then((page) => {
      if (cancelled) return;
      const natural = page.getViewport({ scale: 1 });
      // ResizeObserver's contentRect already excludes the viewport's 24px padding.
      const availableWidth = Math.max(1, size.width);
      const availableHeight = Math.max(1, size.height);
      const fitted = fitDocumentPage(
        { width: availableWidth, height: availableHeight },
        { width: natural.width, height: natural.height },
      );
      const fit = fitted ? fitted.width / natural.width : 1;
      const width = availableWidth / natural.width;
      const scale = Math.max(0.1, scaleMode === 'page-fit' ? fit : scaleMode === 'page-width' ? width : scaleMode === 'actual' ? 1 : customScale);
      const pixelRatio = window.devicePixelRatio || 1;
      const geometry = documentCanvasGeometry(
        { width: natural.width, height: natural.height },
        scale,
        pixelRatio,
      );
      if (!geometry) return;
      const cssViewport = page.getViewport({ scale });
      const renderViewport = page.getViewport({ scale: geometry.renderScale });
      const context = canvas.getContext('2d');
      if (!context) return;
      canvas.width = geometry.pixels.width;
      canvas.height = geometry.pixels.height;
      canvas.style.width = `${geometry.css.width}px`;
      canvas.style.height = `${geometry.css.height}px`;
      const textContainer = textLayerRef.current;
      if (textContainer) {
        textContainer.replaceChildren();
        textContainer.style.width = `${cssViewport.width}px`;
        textContainer.style.height = `${cssViewport.height}px`;
        textContainer.style.setProperty('--scale-factor', String(scale));
        textLayer = new TextLayer({
          textContentSource: page.streamTextContent(),
          container: textContainer,
          viewport: cssViewport,
        });
        void textLayer.render();
      }
      renderedScaleRef.current = scale;
      setRenderedScale(scale);
      renderTask = page.render({ canvas, canvasContext: context, viewport: renderViewport });
      return renderTask.promise.then(() => { if (!cancelled) onReady?.(); });
    }).catch((reason: unknown) => {
      if (!cancelled && (reason as { name?: string }).name !== 'RenderingCancelledException') {
        setError(reason instanceof Error ? reason.message : t('Could not render this PDF page.'));
        onReady?.();
      }
    });

    return () => {
      cancelled = true;
      renderTask?.cancel();
      textLayer?.cancel();
    };
  }, [customScale, document, onReady, pageNumber, scaleMode, size]);

  const setZoomScale = useCallback((scale: number) => {
    setCustomScale(Math.min(8, Math.max(0.25, scale)));
    setScaleMode('custom');
  }, []);

  const changeZoom = useCallback((factor: number) => {
    setZoomScale(renderedScaleRef.current * factor);
  }, [setZoomScale]);

  const zoomControls = useMemo<ViewerZoomControls>(() => ({
    fitToWindow: () => setScaleMode('page-fit'),
    fitActual: () => setScaleMode('actual'),
    zoomIn: () => changeZoom(1.2),
    zoomOut: () => changeZoom(1 / 1.2),
    setZoomScale,
    subscribeZoomScale(listener) {
      zoomListenersRef.current.add(listener);
      listener(renderedScaleRef.current);
      return () => zoomListenersRef.current.delete(listener);
    },
  }), [changeZoom, setZoomScale]);

  useEffect(() => {
    onZoomControlsChange?.(zoomControls);
    return () => onZoomControlsChange?.(null);
  }, [onZoomControlsChange, zoomControls]);

  useEffect(() => {
    onZoomPercentChange?.(Math.round(renderedScale * 100));
    for (const listener of zoomListenersRef.current) listener(renderedScale);
  }, [onZoomPercentChange, renderedScale]);

  const pageCount = document?.numPages ?? 0;
  return (
    <div data-pdf-viewer>
      <DocumentViewerShell
        viewportRef={viewportRef}
        error={error}
        pageNumber={pageNumber}
        pageCount={pageCount}
        onPreviousPage={() => setPageNumber((page) => page - 1)}
        onNextPage={() => setPageNumber((page) => page + 1)}
        navigationLabel="PDF"
      >
        <div className={styles.page}>
          <canvas ref={canvasRef} />
          <div ref={textLayerRef} className={styles.textLayer} />
        </div>
      </DocumentViewerShell>
    </div>
  );
}
