/**
 * Library switcher — sits at the top of the sidebar.
 * Shows current library name with a chevron. Click opens a dropdown
 * to switch between recent/pinned libraries or create/open a new one.
 */

import { useEffect, useRef, useState } from 'react';
import {
  IconChevronDown,
  IconLibrary,
  IconPlus,
  IconFolderOpen,
  IconPinFilled,
  IconCheck,
} from '@tabler/icons-react';
import { useLibraryStore } from '../../../state/libraryStore';
import { save as showSaveDialog } from '#desktop/api';
import styles from './LibrarySwitcher.module.css';

export function LibrarySwitcher() {
  const { libraries, currentPath, switching, loadConfig, switchLibrary, openLibrary, createLibrary } = useLibraryStore();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // Load config on mount
  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  // Close dropdown on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const currentLib = libraries.find((l) => l.isCurrent);
  const displayName = currentLib?.name ?? 'Library';

  const pinned = libraries.filter((l) => l.isPinned);
  const unpinned = libraries.filter((l) => !l.isPinned);

  const handleSwitch = async (path: string) => {
    setOpen(false);
    if (path !== currentPath) {
      await switchLibrary(path);
    }
  };

  const handleOpen = async () => {
    setOpen(false);
    await openLibrary();
  };

  const handleNew = async () => {
    setOpen(false);
    // Show save dialog — user picks location and names the .library folder
    const savePath = await showSaveDialog({
      title: 'Create New Library',
      defaultPath: 'My Library.library',
      properties: ['createDirectory'],
    });
    if (!savePath) return;
    // Extract name from the chosen path (e.g. "/Users/x/My Library.library" → "My Library")
    const filename = savePath.split('/').pop() ?? 'Library';
    const name = filename.replace(/\.library$/, '');
    const dir = savePath.substring(0, savePath.lastIndexOf('/'));
    await createLibrary(name, dir);
  };

  return (
    <div className={styles.root} ref={ref}>
      <button
        className={styles.trigger}
        onClick={() => setOpen((v) => !v)}
        disabled={switching}
      >
        <IconLibrary size={14} className={styles.triggerIcon} />
        <span className={styles.triggerName}>{switching ? 'Switching…' : displayName}</span>
        <IconChevronDown size={12} className={`${styles.triggerChevron} ${open ? styles.triggerChevronOpen : ''}`} />
      </button>

      {open && (
        <div className={styles.dropdown}>
          {/* Pinned libraries */}
          {pinned.map((lib) => (
            <button
              key={lib.path}
              className={`${styles.dropdownItem} ${!lib.exists ? styles.dropdownItemMissing : ''}`}
              onClick={() => lib.exists && handleSwitch(lib.path)}
              disabled={!lib.exists}
            >
              <IconPinFilled size={12} className={styles.dropdownItemPin} />
              <span className={styles.dropdownItemLabel}>
                {lib.name}{!lib.exists && ' (missing)'}
              </span>
              {lib.isCurrent && lib.exists && <IconCheck size={14} className={styles.dropdownItemCheck} />}
            </button>
          ))}

          {/* Separator */}
          {pinned.length > 0 && unpinned.length > 0 && (
            <div className={styles.dropdownSeparator} />
          )}

          {/* Unpinned libraries */}
          {unpinned.map((lib) => (
            <button
              key={lib.path}
              className={`${styles.dropdownItem} ${!lib.exists ? styles.dropdownItemMissing : ''}`}
              onClick={() => lib.exists && handleSwitch(lib.path)}
              disabled={!lib.exists}
            >
              <span className={styles.dropdownItemLabel}>
                {lib.name}{!lib.exists && ' (missing)'}
              </span>
              {lib.isCurrent && lib.exists && <IconCheck size={14} className={styles.dropdownItemCheck} />}
            </button>
          ))}

          {/* Actions */}
          {libraries.length > 0 && <div className={styles.dropdownSeparator} />}
          <button className={styles.dropdownItem} onClick={handleNew}>
            <IconPlus size={14} className={styles.dropdownItemIcon} />
            <span className={styles.dropdownItemLabel}>New Library…</span>
          </button>
          <button className={styles.dropdownItem} onClick={handleOpen}>
            <IconFolderOpen size={14} className={styles.dropdownItemIcon} />
            <span className={styles.dropdownItemLabel}>Open Library…</span>
          </button>
        </div>
      )}
    </div>
  );
}
