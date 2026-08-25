import { useEffect, useMemo, useState } from 'react';
import { getSlides, getSlideSize, loadPresentation, type PresentationData, type SlideData } from '@office-kit/pptx';
import type { CSSProperties } from 'react';
import { renderSlideToSvg } from '@office-kit/pptx-preview';
import { DocumentViewerShell } from './DocumentViewerShell';
import styles from './PptxViewer.module.css';

interface Props { src: string; onReady?: () => void }

function sanitizeSvg(svg: string) {
  const parsed = new DOMParser().parseFromString(svg, 'image/svg+xml');
  parsed.querySelectorAll('script, iframe, object, embed').forEach((node) => node.remove());
  parsed.querySelectorAll('*').forEach((element) => {
    for (const attribute of [...element.attributes]) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim().toLowerCase();
      if (name.startsWith('on') || ((name === 'href' || name.endsWith(':href')) && value.startsWith('javascript:'))) {
        element.removeAttribute(attribute.name);
      }
    }
  });
  return new XMLSerializer().serializeToString(parsed.documentElement);
}

export function PptxViewer({ src, onReady }: Props) {
  const [presentation, setPresentation] = useState<PresentationData | null>(null);
  const [slides, setSlides] = useState<readonly SlideData[]>([]);
  const [pageNumber, setPageNumber] = useState(1);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const abort = new AbortController();
    setPresentation(null);
    setSlides([]);
    setPageNumber(1);
    setError(null);
    void fetch(src, { signal: abort.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`PPTX request failed (${response.status})`);
        return response.arrayBuffer();
      })
      .then((buffer) => loadPresentation(new Uint8Array(buffer)))
      .then((loaded) => {
        if (abort.signal.aborted) return;
        const nextSlides = getSlides(loaded);
        setPresentation(loaded);
        setSlides(nextSlides);
        onReady?.();
      })
      .catch((reason: unknown) => {
        if (!abort.signal.aborted) {
          setError(reason instanceof Error ? reason.message : 'Could not open this presentation.');
          onReady?.();
        }
      });
    return () => abort.abort();
  }, [onReady, src]);

  const svg = useMemo(() => {
    if (!presentation || slides.length === 0) return '';
    return sanitizeSvg(renderSlideToSvg(presentation, slides[pageNumber - 1]));
  }, [pageNumber, presentation, slides]);
  const slideSize = presentation ? getSlideSize(presentation) : null;
  const aspectRatio = slideSize ? `${Number(slideSize.width)} / ${Number(slideSize.height)}` : '16 / 9';

  return (
    <DocumentViewerShell
      error={error}
      pageNumber={pageNumber}
      pageCount={slides.length}
      onPreviousPage={() => setPageNumber((page) => page - 1)}
      onNextPage={() => setPageNumber((page) => page + 1)}
      navigationLabel="presentation"
    >
      {svg ? <div className={styles.slide} style={{ aspectRatio } as CSSProperties} data-pptx-slide dangerouslySetInnerHTML={{ __html: svg }} /> : null}
    </DocumentViewerShell>
  );
}
