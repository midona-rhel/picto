import { marked } from 'marked';
import { useEffect, useRef, useState } from 'react';
import { DocumentViewerShell } from './DocumentViewerShell';
import styles from './TextDocumentViewer.module.css';

interface Props {
  src: string;
  mimeType: string;
  onReady?: () => void;
}

function sanitizeMarkup(markup: string): DocumentFragment {
  const parsed = new DOMParser().parseFromString(markup, 'text/html');
  parsed.querySelectorAll('script, style, iframe, object, embed, link, meta, base, form').forEach((node) => node.remove());
  parsed.querySelectorAll('*').forEach((element) => {
    for (const attribute of [...element.attributes]) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim().toLowerCase();
      if (name.startsWith('on') || ((name === 'href' || name === 'src') && value.startsWith('javascript:'))) {
        element.removeAttribute(attribute.name);
      }
    }
  });
  const fragment = document.createDocumentFragment();
  fragment.append(...parsed.body.childNodes);
  return fragment;
}

function formattedPlainText(text: string, mimeType: string): string {
  if (mimeType !== 'application/json') return text;
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

export function TextDocumentViewer({ src, mimeType, onReady }: Props) {
  const pageRef = useRef<HTMLElement>(null);
  const [plainText, setPlainText] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const abort = new AbortController();
    setPlainText('');
    setError(null);
    void (async () => {
      const response = await fetch(src, { signal: abort.signal });
      if (!response.ok) throw new Error(`Document request failed (${response.status})`);
      return mimeType === 'application/rtf' || mimeType === 'text/rtf'
        ? await response.arrayBuffer()
        : await response.text();
    })()
      .then(async (content) => {
        if (abort.signal.aborted) return;
        const page = pageRef.current;
        if (!page) return;
        page.replaceChildren();
        if (content instanceof ArrayBuffer) {
          const { RTFJS } = await import('rtf.js');
          RTFJS.loggingEnabled(false);
          const rendered = await new RTFJS.Document(content, {}).render();
          if (!abort.signal.aborted) {
            page.append(...rendered);
            onReady?.();
          }
          return;
        }
        if (mimeType === 'text/markdown') {
          page.append(sanitizeMarkup(await marked.parse(content)));
          onReady?.();
          return;
        }
        setPlainText(formattedPlainText(content, mimeType));
        onReady?.();
      })
      .catch((reason: unknown) => {
        if (!abort.signal.aborted) {
          setError(reason instanceof Error ? reason.message : 'Could not open this document.');
          onReady?.();
        }
      });
    return () => abort.abort();
  }, [mimeType, onReady, src]);

  return (
    <DocumentViewerShell error={error} pageNumber={1} pageCount={1} navigationLabel="document">
      <article ref={pageRef} className={styles.page} data-text-document-page>
        {plainText ? <pre className={styles.plainText}>{plainText}</pre> : null}
      </article>
    </DocumentViewerShell>
  );
}
