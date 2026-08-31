import initDjvu, { WasmDocument, wasmSimd128Supported } from 'djvu-rs';
import scalarWasmUrl from '/node_modules/djvu-rs/scalar/djvu_rs_bg.wasm?url';
import simdWasmUrl from '/node_modules/djvu-rs/simd128/djvu_rs_bg.wasm?url';
import { useEffect, useRef, useState } from 'react';
import { DocumentViewerShell } from './DocumentViewerShell';
import styles from './DjvuViewer.module.css';
import { t } from '../../../i18n';

interface Props { src: string; onReady?: () => void }

export function DjvuViewer({ src, onReady }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const documentRef = useRef<WasmDocument | null>(null);
  const [pageNumber, setPageNumber] = useState(1);
  const [pageCount, setPageCount] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const abort = new AbortController();
    setPageNumber(1);
    setPageCount(0);
    setError(null);
    void Promise.all([
      initDjvu(wasmSimd128Supported() ? simdWasmUrl : scalarWasmUrl),
      fetch(src, { signal: abort.signal }).then((response) => {
        if (!response.ok) throw new Error(`DjVu request failed (${response.status})`);
        return response.arrayBuffer();
      }),
    ]).then(([, buffer]) => {
      if (abort.signal.aborted) return;
      const document = WasmDocument.from_bytes(new Uint8Array(buffer));
      documentRef.current = document;
      setPageCount(document.page_count());
    }).catch((reason: unknown) => {
      if (!abort.signal.aborted) {
        setError(reason instanceof Error ? reason.message : t('Could not open this DjVu document.'));
        onReady?.();
      }
    });
    return () => {
      abort.abort();
      documentRef.current?.free();
      documentRef.current = null;
    };
  }, [onReady, src]);

  useEffect(() => {
    const document = documentRef.current;
    const canvas = canvasRef.current;
    if (!document || !canvas || pageCount === 0) return;
    try {
      const page = document.page(pageNumber - 1);
      try {
        const dpi = Math.max(96, page.dpi());
        const width = page.width_at(dpi);
        const height = page.height_at(dpi);
        const pixels = new Uint8ClampedArray(page.render(dpi));
        canvas.width = width;
        canvas.height = height;
        canvas.getContext('2d')?.putImageData(new ImageData(pixels, width, height), 0, 0);
        onReady?.();
      } finally {
        page.free();
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t('Could not render this DjVu page.'));
      onReady?.();
    }
  }, [onReady, pageCount, pageNumber]);

  return (
    <DocumentViewerShell
      error={error}
      pageNumber={pageNumber}
      pageCount={pageCount}
      onPreviousPage={() => setPageNumber((page) => page - 1)}
      onNextPage={() => setPageNumber((page) => page + 1)}
      navigationLabel="DjVu"
    >
      <canvas ref={canvasRef} className={styles.page} data-djvu-page />
    </DocumentViewerShell>
  );
}
