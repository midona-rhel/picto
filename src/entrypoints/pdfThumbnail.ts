import { GlobalWorkerOptions, getDocument } from 'pdfjs-dist/legacy/build/pdf.mjs';
import workerUrl from 'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url';
import { documentCanvasGeometry } from '../features/viewer/document/documentPageGeometry';

declare global {
  interface Window {
    __pictoPdfThumbnail?: { ready?: boolean; error?: string; width?: number; height?: number };
  }
}

GlobalWorkerOptions.workerSrc = workerUrl;

async function render(): Promise<void> {
  const src = new URLSearchParams(location.search).get('src');
  if (!src) throw new Error('Missing PDF source.');
  const response = await fetch(src);
  if (!response.ok) throw new Error(`PDF request failed (${response.status}).`);
  const pdfDocument = await getDocument({ data: new Uint8Array(await response.arrayBuffer()) }).promise;
  const page = await pdfDocument.getPage(1);
  const natural = page.getViewport({ scale: 1 });
  const scale = Math.min(800 / natural.width, 800 / natural.height, 2);
  const geometry = documentCanvasGeometry(
    { width: natural.width, height: natural.height },
    scale,
    window.devicePixelRatio || 1,
  );
  if (!geometry) throw new Error('Could not calculate PDF thumbnail geometry.');
  const viewport = page.getViewport({ scale: geometry.renderScale });
  const canvas = document.querySelector<HTMLCanvasElement>('#page');
  const context = canvas?.getContext('2d', { alpha: false });
  if (!canvas || !context) throw new Error('Could not create the PDF thumbnail canvas.');
  canvas.width = geometry.pixels.width;
  canvas.height = geometry.pixels.height;
  canvas.style.width = `${geometry.css.width}px`;
  canvas.style.height = `${geometry.css.height}px`;
  await page.render({ canvas, canvasContext: context, viewport }).promise;
  window.__pictoPdfThumbnail = { ready: true, width: canvas.width, height: canvas.height };
}

void render().catch((error: unknown) => {
  window.__pictoPdfThumbnail = {
    error: error instanceof Error ? error.message : 'Could not render this PDF.',
  };
});

export {};
