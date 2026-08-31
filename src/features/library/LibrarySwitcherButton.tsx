import { useCallback, useEffect, useRef, useState } from 'react';
import { IconSelector } from '@tabler/icons-react';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { listen } from '../../platform/ipc';
import { LibrarySwitcherPopover } from './LibrarySwitcherPopover';
import { LibraryAvatar, type LibraryAppearance } from './LibraryAvatar';
import styles from './LibrarySwitcherButton.module.css';

function libraryDisplayName(path: string | null): string {
  if (!path) return 'No library';
  const last = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  return last.endsWith('.library') ? last.slice(0, -'.library'.length) : last;
}

/// Current-library button at the top of the sidebar.
/// Shows the open library's name; clicking opens the Library Manager.
export function LibrarySwitcherButton() {
  const [name, setName] = useState<string>('');
  const [meta, setMeta] = useState<LibraryAppearance>({ icon: null, color: null, imageHash: null });
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  const refresh = useCallback(() => {
    (window as any).picto.library
      .getConfig()
      .then((config: any) => {
        setName(libraryDisplayName(config.currentPath));
        const m = config.currentPath ? config.libraryMeta?.[config.currentPath] : null;
        setMeta({
          icon: m?.icon ?? null,
          color: m?.color ?? null,
          imageHash: m?.imageHash ?? null,
          imageFocusX: m?.imageFocusX ?? null,
          imageFocusY: m?.imageFocusY ?? null,
          imageZoomPercent: m?.imageZoomPercent ?? null,
          libraryPath: config.currentPath ?? null,
        });
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    const unlistens: Array<() => void> = [];
    void listen('library-switched', refresh).then((un) => unlistens.push(un));
    void listen('library-meta-changed', refresh).then((un) => unlistens.push(un));
    return () => unlistens.forEach((un) => un());
  }, [refresh]);

  const ready = name.length > 0;

  return (
    <>
      <KbdTooltip label="Switch Library">
        <button
          ref={buttonRef}
          className={styles.button}
          data-help-id="sidebar-library-switcher"
          data-loading={!ready || undefined}
          data-open={anchor != null || undefined}
          aria-busy={!ready}
          aria-expanded={anchor != null}
          disabled={!ready}
          onClick={() =>
            ready && setAnchor((prev) =>
              prev ? null : buttonRef.current?.getBoundingClientRect() ?? null,
            )
          }
        >
          <svg className={styles.highlight} aria-hidden="true" focusable="false">
            <defs>
              <mask id="library-switcher-highlight-mask">
                <rect width="100%" height="100%" fill="black" />
                <circle cx="24" cy="24" r="24" fill="white" />
                <rect className={styles.highlightBar} fill="white" />
                <rect className={styles.highlightBarJoin} fill="white" />
              </mask>
            </defs>
            <rect
              className={styles.highlightFill}
              width="100%"
              height="100%"
              mask="url(#library-switcher-highlight-mask)"
            />
          </svg>
          <LibraryAvatar appearance={meta} size={39} className={styles.icon} />
          <span className={styles.name}>{name || '\u00a0'}</span>
          <span className={styles.chevron}><IconSelector size={14} stroke={1.25} /></span>
        </button>
      </KbdTooltip>
      {anchor ? (
        <LibrarySwitcherPopover
          anchor={anchor}
          trigger={buttonRef.current}
          onClose={() => setAnchor(null)}
        />
      ) : null}
    </>
  );
}
