import JSZip from 'jszip';
import { useEffect, useMemo, useState } from 'react';
import { DocumentViewerShell } from './DocumentViewerShell';
import styles from './ArchiveDocumentViewer.module.css';
import { getLocale, t } from '../../../i18n';

interface Props { src: string; kind: 'epub' | 'cbz'; onReady?: () => void }
interface EpubChapter { path: string; title: string }

const IMAGE_EXTENSIONS = /\.(?:avif|bmp|gif|jpe?g|png|svg|webp)$/i;

function naturalCompare(left: string, right: string) {
  return left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' });
}

function dirname(path: string) {
  return path.includes('/') ? path.slice(0, path.lastIndexOf('/') + 1) : '';
}

function resolveArchivePath(base: string, relative: string) {
  const url = new URL(relative, `https://archive.invalid/${base}`);
  return decodeURIComponent(url.pathname.slice(1));
}

function localElements(root: ParentNode, name: string) {
  return [...root.querySelectorAll('*')].filter((element) => element.localName === name);
}

async function readEpubSpine(zip: JSZip) {
  const containerText = await zip.file('META-INF/container.xml')?.async('text');
  if (!containerText) throw new Error('This EPUB has no container manifest.');
  const container = new DOMParser().parseFromString(containerText, 'application/xml');
  const rootfile = localElements(container, 'rootfile')[0]?.getAttribute('full-path');
  if (!rootfile) throw new Error('This EPUB has no package document.');
  const packageText = await zip.file(rootfile)?.async('text');
  if (!packageText) throw new Error('The EPUB package document is missing.');
  const packageDocument = new DOMParser().parseFromString(packageText, 'application/xml');
  const base = dirname(rootfile);
  const manifest = new Map(localElements(packageDocument, 'item').map((item) => [
    item.getAttribute('id') ?? '',
    { href: item.getAttribute('href') ?? '', mediaType: item.getAttribute('media-type') ?? '' },
  ]));
  return localElements(packageDocument, 'itemref').flatMap<EpubChapter>((item, index) => {
    const entry = manifest.get(item.getAttribute('idref') ?? '');
    if (!entry || !/xhtml|html/i.test(entry.mediaType)) return [];
    return [{ path: resolveArchivePath(base, entry.href), title: t("Chapter {value0}", { value0: index + 1 }) }];
  });
}

function sanitizeEpubDocument(markup: string) {
  const parsed = new DOMParser().parseFromString(markup, 'text/html');
  parsed.querySelectorAll('script, iframe, object, embed, form, link, meta, base').forEach((node) => node.remove());
  parsed.querySelectorAll('*').forEach((element) => {
    for (const attribute of [...element.attributes]) {
      if (attribute.name.toLowerCase().startsWith('on')) element.removeAttribute(attribute.name);
    }
  });
  return parsed;
}

export function ArchiveDocumentViewer({ src, kind, onReady }: Props) {
  const locale = getLocale();
  const [zip, setZip] = useState<JSZip | null>(null);
  const [entries, setEntries] = useState<Array<string | EpubChapter>>([]);
  const [pageNumber, setPageNumber] = useState(1);
  const [pageUrl, setPageUrl] = useState('');
  const [chapterMarkup, setChapterMarkup] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const abort = new AbortController();
    setZip(null);
    setEntries([]);
    setPageNumber(1);
    setError(null);
    void fetch(src, { signal: abort.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`${kind.toUpperCase()} request failed (${response.status})`);
        return response.arrayBuffer();
      })
      .then((buffer) => JSZip.loadAsync(buffer))
      .then(async (archive) => {
        const nextEntries = kind === 'cbz'
          ? Object.values(archive.files).filter((entry) => !entry.dir && IMAGE_EXTENSIONS.test(entry.name)).map((entry) => entry.name).sort(naturalCompare)
          : await readEpubSpine(archive);
        if (nextEntries.length === 0) throw new Error(`This ${kind.toUpperCase()} has no readable pages.`);
        if (!abort.signal.aborted) {
          setZip(archive);
          setEntries(nextEntries);
        }
      })
      .catch((reason: unknown) => {
        if (!abort.signal.aborted) {
          setError(reason instanceof Error ? reason.message : t('Could not open this {value0}.', { value0: kind.toUpperCase() }));
          onReady?.();
        }
      });
    return () => abort.abort();
  }, [kind, onReady, src]);

  useEffect(() => {
    if (!zip || entries.length === 0) return;
    let objectUrl = '';
    let cancelled = false;
    setPageUrl('');
    setChapterMarkup('');
    const entry = entries[pageNumber - 1];
    if (kind === 'cbz') {
      void zip.file(entry as string)?.async('blob').then((blob) => {
        if (cancelled) return;
        objectUrl = URL.createObjectURL(blob);
        setPageUrl(objectUrl);
      });
    } else {
      const chapter = entry as EpubChapter;
      void zip.file(chapter.path)?.async('text').then(async (markup) => {
        const parsed = sanitizeEpubDocument(markup);
        const base = dirname(chapter.path);
        const urls: string[] = [];
        for (const image of [...parsed.images]) {
          const path = resolveArchivePath(base, image.getAttribute('src') ?? '');
          const file = zip.file(path);
          if (!file) { image.remove(); continue; }
          const url = URL.createObjectURL(await file.async('blob'));
          urls.push(url);
          image.src = url;
        }
        if (!cancelled) {
          objectUrl = urls.join('|');
          setChapterMarkup(parsed.body.innerHTML);
          onReady?.();
        } else urls.forEach((url) => URL.revokeObjectURL(url));
      });
    }
    return () => {
      cancelled = true;
      objectUrl.split('|').filter(Boolean).forEach((url) => URL.revokeObjectURL(url));
    };
  }, [entries, kind, onReady, pageNumber, zip]);

  const content = useMemo(() => kind === 'cbz'
    ? (pageUrl ? <img className={styles.comicPage} src={pageUrl} alt={t("Page {value0}", { value0: pageNumber })} onLoad={onReady} onError={onReady} /> : null)
    : (chapterMarkup ? <article className={styles.bookPage} dangerouslySetInnerHTML={{ __html: chapterMarkup }} /> : null),
  [chapterMarkup, kind, locale, pageNumber, pageUrl]);

  return (
    <DocumentViewerShell
      error={error}
      pageNumber={pageNumber}
      pageCount={entries.length}
      onPreviousPage={() => setPageNumber((page) => page - 1)}
      onNextPage={() => setPageNumber((page) => page + 1)}
      navigationLabel={kind === 'cbz' ? 'comic' : 'book'}
    >
      {content}
    </DocumentViewerShell>
  );
}
