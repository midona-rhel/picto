import { GlobalWorkerOptions, getDocument } from 'pdfjs-dist/legacy/build/pdf.mjs';
import workerUrl from 'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url';

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
  const viewport = page.getViewport({ scale });
  const canvas = document.querySelector<HTMLCanvasElement>('#page');
  const context = canvas?.getContext('2d', { alpha: false });
  if (!canvas || !context) throw new Error('Could not create the PDF thumbnail canvas.');
  canvas.width = Math.max(1, Math.ceil(viewport.width));
  canvas.height = Math.max(1, Math.ceil(viewport.height));
  await page.render({ canvas, canvasContext: context, viewport }).promise;
  window.__pictoPdfThumbnail = { ready: true, width: canvas.width, height: canvas.height };
}

void render().catch((error: unknown) => {
  window.__pictoPdfThumbnail = {
    error: error instanceof Error ? error.message : 'Could not render this PDF.',
  };
});

export {};
