import { useEffect, useRef, useState } from 'react';
import { renderAsync } from 'docx-preview';
import { DocumentViewerShell } from './DocumentViewerShell';
import styles from './DocxViewer.module.css';

interface Props { src: string; onReady?: () => void }

export function DocxViewer({ src, onReady }: Props) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const [pageNumber, setPageNumber] = useState(1);
  const [pageCount, setPageCount] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const abort = new AbortController();
    const body = bodyRef.current;
    if (!body) return;
    body.replaceChildren();
    setPageNumber(1);
    setPageCount(0);
    setError(null);
    void fetch(src, { signal: abort.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`DOCX request failed (${response.status})`);
        return response.arrayBuffer();
      })
      .then((buffer) => renderAsync(buffer, body, body, {
        inWrapper: true,
        breakPages: true,
        ignoreLastRenderedPageBreak: false,
        renderHeaders: true,
        renderFooters: true,
        renderFootnotes: true,
        renderEndnotes: true,
        renderComments: false,
        renderAltChunks: false,
        useBase64URL: true,
      }))
      .then(() => {
        if (!abort.signal.aborted) {
          setPageCount(body.querySelectorAll('section.docx').length || 1);
          onReady?.();
        }
      })
      .catch((reason: unknown) => {
        if (!abort.signal.aborted) {
          setError(reason instanceof Error ? reason.message : 'Could not open this DOCX document.');
          onReady?.();
        }
      });
    return () => abort.abort();
  }, [onReady, src]);

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport || pageCount < 2) return;
    const updateCurrentPage = () => {
      const viewportTop = viewport.getBoundingClientRect().top + 24;
      const pages = [...(bodyRef.current?.querySelectorAll<HTMLElement>('section.docx') ?? [])];
      let closest = 0;
      let distance = Number.POSITIVE_INFINITY;
      pages.forEach((page, index) => {
        const nextDistance = Math.abs(page.getBoundingClientRect().top - viewportTop);
        if (nextDistance < distance) { closest = index; distance = nextDistance; }
      });
      setPageNumber(closest + 1);
    };
    viewport.addEventListener('scroll', updateCurrentPage, { passive: true });
    return () => viewport.removeEventListener('scroll', updateCurrentPage);
  }, [pageCount]);

  const goToPage = (nextPage: number) => {
    const page = bodyRef.current?.querySelectorAll<HTMLElement>('section.docx')[nextPage - 1];
    const viewport = viewportRef.current;
    if (page && viewport) {
      viewport.scrollTo({
        top: viewport.scrollTop + page.getBoundingClientRect().top - viewport.getBoundingClientRect().top - 24,
        behavior: 'smooth',
      });
    }
    setPageNumber(nextPage);
  };

  return (
    <DocumentViewerShell
      viewportRef={viewportRef}
      error={error}
      pageNumber={pageNumber}
      pageCount={pageCount}
      onPreviousPage={() => goToPage(pageNumber - 1)}
      onNextPage={() => goToPage(pageNumber + 1)}
      navigationLabel="DOCX"
    >
      <div ref={bodyRef} className={styles.document} data-docx-document />
    </DocumentViewerShell>
  );
}
