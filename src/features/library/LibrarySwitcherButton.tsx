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

/// reference application-style current-library button at the top of the sidebar.
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

  if (!name) return null;

  return (
    <>
      <KbdTooltip label="Switch Library">
        <button
          ref={buttonRef}
          className={styles.button}
          onClick={() =>
            setAnchor((prev) =>
              prev ? null : buttonRef.current?.getBoundingClientRect() ?? null,
            )
          }
        >
          <LibraryAvatar appearance={meta} size={26} className={styles.icon} />
          <span className={styles.name}>{name}</span>
          <span className={styles.chevron}><IconSelector size={14} stroke={1.25} /></span>
        </button>
      </KbdTooltip>
      {anchor ? <LibrarySwitcherPopover anchor={anchor} onClose={() => setAnchor(null)} /> : null}
    </>
  );
}
