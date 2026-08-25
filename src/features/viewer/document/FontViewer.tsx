import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { IconMinus, IconPlus } from '@tabler/icons-react';
import styles from './FontViewer.module.css';
import { useShortcutScope } from '../../../shared/hooks/useShortcutScope';

type FontTab = 'preview' | 'waterfall' | 'glyphs' | 'information';
type PreviewTheme = 'auto' | 'light' | 'dark' | 'purple' | 'yellow';

const TABS: Array<{ id: FontTab; label: string }> = [
  { id: 'preview', label: 'Preview' },
  { id: 'waterfall', label: 'Waterfall' },
  { id: 'glyphs', label: 'Glyphs' },
  { id: 'information', label: 'Information' },
];

const THEMES: PreviewTheme[] = ['auto', 'light', 'dark', 'purple', 'yellow'];
const WATERFALL_SIZES = [72, 64, 56, 48, 38, 30, 24, 20, 16, 14, 12];
const GLYPH_GROUPS = [
  { label: 'Uppercase', value: 'ABCDEFGHIJKLMNOPQRSTUVWXYZ' },
  { label: 'Lowercase', value: 'abcdefghijklmnopqrstuvwxyz' },
  { label: 'Numbers', value: '0123456789' },
  { label: 'Punctuation', value: '.,:;!?@#$%&*()[]{}+-=/\\_\u2014\u2013\u201c\u201d\u2018\u2019' },
];

function fontFormat(mimeType: string) {
  if (mimeType === 'font/collection') return 'TrueType Collection';
  if (mimeType === 'font/otf') return 'OpenType';
  if (mimeType === 'font/woff') return 'Web Open Font Format';
  return 'TrueType';
}

interface Props {
  src: string;
  displayName: string;
  mimeType: string;
  onReady?: () => void;
}

export function FontViewer({ src, displayName, mimeType, onReady }: Props) {
  const [tab, setTab] = useState<FontTab>('preview');
  const [theme, setTheme] = useState<PreviewTheme>('auto');
  const [scale, setScale] = useState(1);
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const family = useMemo(() => `PictoFontPreview-${Math.random().toString(36).slice(2)}`, [src]);

  useEffect(() => {
    let disposed = false;
    let face: FontFace | null = null;
    setStatus('loading');
    void fetch(src)
      .then((response) => {
        if (!response.ok) throw new Error(`Font request failed (${response.status})`);
        return response.arrayBuffer();
      })
      .then((source) => {
        if (disposed) return;
        face = new FontFace(family, source);
        return face.load();
      })
      .then((loaded) => {
        if (!loaded || disposed) return;
        document.fonts.add(loaded);
        setStatus('ready');
        onReady?.();
      })
      .catch(() => {
        if (!disposed) {
          setStatus('error');
          onReady?.();
        }
      });
    return () => {
      disposed = true;
      if (face) document.fonts.delete(face);
    };
  }, [family, onReady, src]);

  useShortcutScope((event) => {
      if (!(event.metaKey || event.ctrlKey)) return;
      if (event.key === '=' || event.key === '+') {
        event.preventDefault();
        setScale((value) => Math.min(3, value + 0.1));
      } else if (event.key === '-') {
        event.preventDefault();
        setScale((value) => Math.max(0.5, value - 0.1));
      } else if (event.key === '0') {
        event.preventDefault();
        setScale(1);
      }
  }, { priority: 70 });

  const previewStyle = { '--preview-font': `'${family}', sans-serif`, '--preview-scale': scale } as CSSProperties;

  return (
    <div className={styles.root} data-font-viewer data-preview-theme={theme} style={previewStyle}>
      <div className={styles.page}>
        <header className={styles.header}>
          <h1>{displayName}</h1>
          <span>{fontFormat(mimeType)}</span>
        </header>

        <div className={styles.toolbar}>
          <div className={styles.tabs} role="tablist" aria-label="Font preview mode">
            {TABS.map((item) => (
              <button key={item.id} type="button" role="tab" aria-selected={tab === item.id} className={styles.tab} onClick={() => setTab(item.id)}>
                {item.label}
              </button>
            ))}
          </div>
          <div className={styles.themes} aria-label="Preview background">
            {THEMES.map((item) => (
              <button key={item} type="button" aria-label={`${item} preview`} aria-pressed={theme === item} className={`${styles.theme} ${styles[item]}`} onClick={() => setTheme(item)}>A</button>
            ))}
          </div>
        </div>

        <main className={styles.content}>
          {status === 'loading' ? <div className={styles.message}>Loading font…</div> : null}
          {status === 'error' ? <div className={styles.message} role="alert">This font cannot be previewed by Chromium.</div> : null}
          {status === 'ready' && tab === 'preview' ? (
            <article className={styles.article} contentEditable suppressContentEditableWarning spellCheck={false}>
              <h2>The quick brown fox jumps over the lazy dog.</h2>
              <p>Typography gives language a visible voice. Edit this sample to test the font with your own words, punctuation, and numbers.</p>
              <p>ABCDEFGHIJKLMNOPQRSTUVWXYZ<br />abcdefghijklmnopqrstuvwxyz<br />0123456789</p>
            </article>
          ) : null}
          {status === 'ready' && tab === 'waterfall' ? (
            <div className={styles.waterfall}>
              {WATERFALL_SIZES.map((size) => <div key={size} contentEditable suppressContentEditableWarning spellCheck={false} style={{ fontSize: `calc(${size}px * var(--preview-scale))` }}>Hamburgefontsiv 0123456789</div>)}
            </div>
          ) : null}
          {status === 'ready' && tab === 'glyphs' ? (
            <div className={styles.glyphs}>
              {GLYPH_GROUPS.map((group) => (
                <section key={group.label}><h2>{group.label}</h2><div className={styles.glyphGrid}>{Array.from(group.value).map((glyph, index) => <span key={`${glyph}-${index}`} title={glyph}>{glyph}</span>)}</div></section>
              ))}
            </div>
          ) : null}
          {tab === 'information' ? (
            <dl className={styles.information}>
              <div><dt>File name</dt><dd>{displayName}</dd></div>
              <div><dt>Format</dt><dd>{fontFormat(mimeType)}</dd></div>
              <div><dt>Preview engine</dt><dd>Chromium FontFace</dd></div>
            </dl>
          ) : null}
        </main>
      </div>

      {tab !== 'information' ? (
        <div className={styles.zoom}>
          <button type="button" aria-label="Decrease font preview size" onClick={() => setScale((value) => Math.max(0.5, value - 0.1))}><IconMinus size={14} /></button>
          <input aria-label="Font preview size" type="range" min="50" max="300" value={Math.round(scale * 100)} onChange={(event) => setScale(Number(event.currentTarget.value) / 100)} />
          <button type="button" aria-label="Increase font preview size" onClick={() => setScale((value) => Math.min(3, value + 0.1))}><IconPlus size={14} /></button>
        </div>
      ) : null}
    </div>
  );
}
