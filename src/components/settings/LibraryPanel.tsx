import { useCallback, useEffect, useState } from 'react';
import { Text, TextInput } from '@mantine/core';
import { IconFolderOpen, IconPlus } from '@tabler/icons-react';
import { useLibraryStore, type LibraryInfo } from '../../stores/libraryStore';
import { save as showSaveDialog, api } from '#desktop/api';
import { TextButton } from '../ui/TextButton';
import { StateBlock } from '../ui/state';
import styles from '../Settings.module.css';

interface CurrentLibraryInfo {
  path: string;
  name: string;
  file_count: number;
}

export function LibraryPanel() {
  const { libraries, loadConfig, createLibrary, openLibrary, removeLibrary, deleteLibrary, togglePin, renameLibrary, relocateLibrary, getLibraryInfo } = useLibraryStore();
  const [currentInfo, setCurrentInfo] = useState<CurrentLibraryInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');

  const loadPanelData = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      await loadConfig();
      const info = await getLibraryInfo();
      setCurrentInfo(info ?? null);
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : 'Failed to load library information');
    } finally {
      setLoading(false);
    }
  }, [loadConfig, getLibraryInfo]);

  useEffect(() => {
    void loadPanelData();
  }, [loadPanelData]);

  const handleCreate = async () => {
    if (!newName.trim()) return;
    const savePath = await showSaveDialog({
      title: 'Choose where to save the library',
      defaultPath: `${newName.trim()}.library`,
      properties: ['createDirectory'],
    });
    if (!savePath) return;
    // Extract directory from the save dialog path
    const dir = savePath.substring(0, savePath.lastIndexOf('/'));
    await createLibrary(newName.trim(), dir);
    setNewName('');
    setCreating(false);
  };

  const handleRevealInFinder = (libPath: string) => {
    api.os.openExternalUrl(`file://${libPath}`);
  };

  if (loading) {
    return <StateBlock variant="loading" title="Loading libraries" compact minHeight={80} />;
  }

  if (loadError) {
    return (
      <StateBlock
        variant="error"
        title="Failed to load libraries"
        description={loadError}
        action={<TextButton onClick={() => void loadPanelData()}>Retry</TextButton>}
      />
    );
  }

  const pinned = libraries.filter((l) => l.isPinned);
  const unpinned = libraries.filter((l) => !l.isPinned);

  return (
    <div>
      {/* Current library info */}
      <div className={styles.panelBlock}>
        <div className={styles.blockTitle}>Current Library</div>
        <div className={styles.blockContent}>
          {currentInfo ? (
            <>
              <div className={styles.labelItem}>
                <label>Name</label>
                <div className={styles.right}>
                  <Text size="sm">{currentInfo.name}</Text>
                </div>
              </div>
              <div className={styles.blockSeparator} />
              <div className={styles.labelItem}>
                <label>Path</label>
                <div className={styles.right}>
                  <Text size="xs" c="dimmed" style={{ wordBreak: 'break-all' }}>
                    {currentInfo.path}
                  </Text>
                </div>
              </div>
              <div className={styles.blockSeparator} />
              <div className={styles.labelItem}>
                <label>Files</label>
                <div className={styles.right}>
                  <Text size="sm">{currentInfo.file_count.toLocaleString()}</Text>
                </div>
              </div>
            </>
          ) : (
            <Text size="sm" c="dimmed">No library open</Text>
          )}
        </div>
      </div>

      {/* Library actions */}
      <div className={styles.panelBlock}>
        <div className={styles.blockTitle}>Manage Libraries</div>
        <div className={styles.blockContent}>
          <div style={{ display: 'flex', gap: 6 }}>
            <TextButton onClick={() => setCreating(true)}>
              <IconPlus size={14} />
              New Library
            </TextButton>
            <TextButton onClick={openLibrary}>
              <IconFolderOpen size={14} />
              Open Library
            </TextButton>
          </div>

          {creating && (
            <div style={{ marginTop: 12 }}>
              <input
                type="text"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="Library name"
                onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
                style={{
                  width: '100%',
                  padding: '6px 10px',
                  borderRadius: 6,
                  border: '1px solid var(--color-border-primary)',
                  background: 'transparent',
                  color: 'var(--color-text-primary)',
                  fontSize: 'var(--font-size-md)',
                  marginBottom: 8,
                }}
                autoFocus
              />
              <div style={{ display: 'flex', gap: 6 }}>
                <TextButton compact onClick={handleCreate} disabled={!newName.trim()}>
                  Create
                </TextButton>
                <TextButton compact onClick={() => { setCreating(false); setNewName(''); }}>
                  Cancel
                </TextButton>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Recent libraries */}
      {libraries.length > 0 && (
        <div className={styles.panelBlock}>
          <div className={styles.blockTitle}>Recent Libraries</div>
          <div className={styles.blockContent}>
            {pinned.length > 0 && (
              <>
                {pinned.map((lib) => (
                  <LibraryRow
                    key={lib.path}
                    lib={lib}
                    onTogglePin={togglePin}
                    onRemove={removeLibrary}
                    onDelete={deleteLibrary}
                    onReveal={handleRevealInFinder}
                    onRename={renameLibrary}
                    onRelocate={relocateLibrary}
                  />
                ))}
                {unpinned.length > 0 && <div className={styles.blockSeparator} />}
              </>
            )}
            {unpinned.map((lib) => (
              <LibraryRow
                key={lib.path}
                lib={lib}
                onTogglePin={togglePin}
                onRemove={removeLibrary}
                onDelete={deleteLibrary}
                onReveal={handleRevealInFinder}
                onRename={renameLibrary}
                onRelocate={relocateLibrary}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function LibraryRow({
  lib,
  onTogglePin,
  onRemove,
  onDelete,
  onReveal,
  onRename,
  onRelocate,
}: {
  lib: LibraryInfo;
  onTogglePin: (path: string) => void;
  onRemove: (path: string) => void;
  onDelete: (path: string) => void;
  onReveal: (path: string) => void;
  onRename: (path: string, newName: string) => Promise<void>;
  onRelocate: (oldPath: string) => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState(lib.name);
  const [renameError, setRenameError] = useState<string | null>(null);


  const startEditing = () => {
    setEditName(lib.name);
    setRenameError(null);
    setEditing(true);
  };

  const confirmRename = async () => {
    const trimmed = editName.trim();
    if (!trimmed || trimmed === lib.name) {
      setEditing(false);
      return;
    }
    try {
      await onRename(lib.path, trimmed);
      setEditing(false);
      setRenameError(null);
    } catch (err: unknown) {
      setRenameError(err instanceof Error ? err.message : 'Rename failed');
    }
  };

  const isMissing = !lib.exists;

  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '4px 0' }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        {editing ? (
          <TextInput
            size="xs"
            value={editName}
            onChange={(e) => setEditName(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') confirmRename();
              if (e.key === 'Escape') setEditing(false);
            }}
            onBlur={() => setEditing(false)}
            error={renameError}
            autoFocus
          />
        ) : (
          <Text
            size="sm"
            fw={lib.isCurrent ? 500 : 400}
            c={isMissing ? 'dimmed' : undefined}
            style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
          >
            {lib.isPinned && '\u{1F4CC} '}
            {lib.name}
            {lib.isCurrent && ' (current)'}
            {isMissing && ' (missing)'}
          </Text>
        )}
        <Text size="xs" c="dimmed" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {lib.path}
        </Text>
      </div>
      <div style={{ display: 'flex', gap: 4, flexWrap: 'nowrap' }}>
        <TextButton compact onClick={() => onRelocate(lib.path)}>
          Relocate
        </TextButton>
        {isMissing ? (
          <TextButton compact onClick={() => onRemove(lib.path)}>
            Remove
          </TextButton>
        ) : (
          <>
            <TextButton compact onClick={startEditing}>
              Rename
            </TextButton>
            <TextButton compact onClick={() => onTogglePin(lib.path)}>
              {lib.isPinned ? 'Unpin' : 'Pin'}
            </TextButton>
            <TextButton compact onClick={() => onReveal(lib.path)}>
              Reveal
            </TextButton>
            {!lib.isCurrent && (
              <>
                <TextButton compact onClick={() => onRemove(lib.path)}>
                  Remove
                </TextButton>
                <TextButton compact danger onClick={() => onDelete(lib.path)}>
                  Delete
                </TextButton>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
