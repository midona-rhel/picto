import { createRoot } from 'react-dom/client';
import { MantineProvider } from '@mantine/core';
import { DetailMediaRenderer } from '../features/viewer/document/DetailMediaRenderer';
import { publishPlatform } from '../shared/lib/platform';
import '../app/globals.css';

publishPlatform();

declare global {
  interface Window {
    __pictoDocumentThumbnail?: { ready?: boolean; error?: string; width?: number; height?: number };
  }
}

const query = new URLSearchParams(location.search);
const hash = query.get('hash') ?? '';
const mimeType = query.get('mime') ?? '';
const root = document.getElementById('root');

function contentIsReady() {
  const docx = document.querySelector('section.docx');
  const pptx = document.querySelector('[data-pptx-slide] svg');
  const text = document.querySelector('[data-text-document-page]');
  const article = document.querySelector('[data-document-viewer] article');
  const image = document.querySelector<HTMLImageElement>('[data-document-viewer] img');
  const canvas = document.querySelector<HTMLCanvasElement>('[data-djvu-page]');
  return Boolean(
    docx
    || pptx
    || (text && (text.textContent?.trim() || text.children.length > 0))
    || (article && (article.textContent?.trim() || article.children.length > 0))
    || (image?.complete && image.naturalWidth > 0)
    || (canvas && canvas.width > 0 && canvas.height > 0),
  );
}

async function settle() {
  const deadline = performance.now() + 20_000;
  while (performance.now() < deadline) {
    const error = document.querySelector<HTMLElement>('[role="alert"]')?.innerText;
    if (error) throw new Error(error);
    if (contentIsReady()) {
      await document.fonts.ready;
      await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      window.__pictoDocumentThumbnail = { ready: true, width: 800, height: 752 };
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error('Document thumbnail renderer did not settle.');
}

if (!root || !hash || !mimeType) {
  window.__pictoDocumentThumbnail = { error: 'Missing document thumbnail parameters.' };
} else {
  createRoot(root).render(
    <MantineProvider forceColorScheme="dark" cssVariablesSelector=":root:root">
      <DetailMediaRenderer hash={hash} mimeType={mimeType} />
    </MantineProvider>,
  );
  void settle().catch((reason: unknown) => {
    window.__pictoDocumentThumbnail = { error: reason instanceof Error ? reason.message : 'Document thumbnail failed.' };
  });
}
