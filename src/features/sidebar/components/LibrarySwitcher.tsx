/**
 * Library switcher — sits at the top of the sidebar.
 * Shows current library name with a chevron. Click opens a dropdown
 * to switch between recent/pinned libraries or create/open a new one.
 */

import { useEffect, useRef, useState } from 'react';
import { ActionIcon, Group, Modal, Stack, Text, TextInput } from '@mantine/core';
import { glassModalStyles } from '../../../shared/styles/glassModal';
import { TextButton } from '../../../shared/components/TextButton';
import {
  IconSelector,
  IconPlus,
  IconFolderOpen,
  IconCheck,
  IconSettings,
} from '@tabler/icons-react';
import { DynamicIcon, DEFAULT_FOLDER_ICON } from '../../smart-folders/components/iconRegistry';
import { IconPicker } from '../../smart-folders/components/IconPicker';
import { FolderColorPicker } from '../../smart-folders/components/FolderColorPicker';
import { useLibraryStore } from '../../../state/libraryStore';
import { save as showSaveDialog } from '#desktop/api';
import styles from './LibrarySwitcher.module.css';

export function LibrarySwitcher() {
  const { libraries, currentPath, switching, loadConfig, switchLibrary, openLibrary, createLibrary, setLibraryIcon, setLibraryColor, renameLibrary, relocateLibrary } = useLibraryStore();
  const [open, setOpen] = useState(false);
  const [editModalOpen, setEditModalOpen] = useState(false);
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
    // Extract name from the chosen path (e.g. "<home>/x/My Library.library" → "My Library")
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
        <DynamicIcon
          name={currentLib?.icon ?? 'IconLibrary'}
          size={14}
          color={currentLib?.color ?? 'currentColor'}
        />
        <span className={styles.triggerName}>{switching ? 'Switching…' : displayName}</span>
        <IconSelector size={12} className={styles.triggerChevron} />
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
              <DynamicIcon name={lib.icon ?? 'IconLibrary'} size={14} color={lib.color ?? 'currentColor'} />
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
              <DynamicIcon name={lib.icon ?? 'IconLibrary'} size={14} color={lib.color ?? 'currentColor'} />
              <span className={styles.dropdownItemLabel}>
                {lib.name}{!lib.exists && ' (missing)'}
              </span>
              {lib.isCurrent && lib.exists && <IconCheck size={14} className={styles.dropdownItemCheck} />}
            </button>
          ))}

          {/* Actions */}
          {libraries.length > 0 && <div className={styles.dropdownSeparator} />}
          {currentLib && currentPath && (
            <button className={styles.dropdownItem} onClick={() => { setOpen(false); setEditModalOpen(true); }}>
              <IconSettings size={14} className={styles.dropdownItemIcon} />
              <span className={styles.dropdownItemLabel}>Edit Library…</span>
            </button>
          )}
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
      <EditLibraryModal
        opened={editModalOpen && !!currentLib && !!currentPath}
        name={currentLib?.name ?? ''}
        path={currentPath ?? ''}
        icon={currentLib?.icon ?? null}
        color={currentLib?.color ?? null}
        onRename={(newName) => currentPath ? renameLibrary(currentPath, newName) : Promise.resolve()}
        onRelocate={() => currentPath ? relocateLibrary(currentPath) : Promise.resolve()}
        onIconChange={(icon) => currentPath ? setLibraryIcon(currentPath, icon) : Promise.resolve()}
        onColorChange={(color) => currentPath ? setLibraryColor(currentPath, color) : Promise.resolve()}
        onClose={() => setEditModalOpen(false)}
      />
    </div>
  );
}

function EditLibraryModal({ opened, name, path, icon, color, onRename, onRelocate, onIconChange, onColorChange, onClose }: {
  opened: boolean;
  name: string;
  path: string;
  icon: string | null;
  color: string | null;
  onRename: (name: string) => Promise<void>;
  onRelocate: () => Promise<void>;
  onIconChange: (icon: string | null) => Promise<void>;
  onColorChange: (color: string | null) => Promise<void>;
  onClose: () => void;
}) {
  const [editName, setEditName] = useState(name);

  useEffect(() => {
    if (opened) setEditName(name);
  }, [opened, name]);

  const handleRename = async () => {
    const trimmed = editName.trim();
    if (trimmed && trimmed !== name) {
      await onRename(trimmed);
    }
  };

  const handleRelocate = async () => {
    await onRelocate();
    onClose();
  };

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title="Edit Library"
      centered
      size="lg"
      styles={{
        ...glassModalStyles,
        title: { fontWeight: 600, fontSize: 'var(--mantine-font-size-lg)' },
        body: { padding: 'var(--mantine-spacing-lg)' },
      }}
    >
      <Stack gap="md">
        <Group grow align="flex-start">
          <TextInput
            label="Name"
            placeholder="Library name..."
            value={editName}
            onChange={(e) => setEditName(e.currentTarget.value)}
            onBlur={handleRename}
            onKeyDown={(e) => { if (e.key === 'Enter') handleRename(); }}
            size="sm"
          />
        </Group>

        <div>
          <Text size="sm" fw={500} mb={6}>Location</Text>
          <Text size="xs" c="dimmed" style={{ wordBreak: 'break-all' }}>{path}</Text>
        </div>

        <Group gap="xl">
          <div>
            <Text size="sm" fw={500} mb={6}>Icon</Text>
            <IconPicker value={icon} onChange={onIconChange}>
              <ActionIcon variant="light" color="gray" size="lg">
                <DynamicIcon name={icon ?? DEFAULT_FOLDER_ICON} size={18} color={color ?? undefined} />
              </ActionIcon>
            </IconPicker>
          </div>
          <div>
            <Text size="sm" fw={500} mb={6}>Color</Text>
            <FolderColorPicker value={color} onChange={onColorChange} />
          </div>
        </Group>

        <Group justify="flex-end" mt="xs">
          <TextButton onClick={handleRelocate}>Relocate…</TextButton>
          <TextButton onClick={onClose}>Done</TextButton>
        </Group>
      </Stack>
    </Modal>
  );
}
