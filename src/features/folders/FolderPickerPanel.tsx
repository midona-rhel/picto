/**
 * FolderPickerPanel — centered modal for managing folder membership.
 *
 * Shows current folder memberships as chips (with remove), then a full
 * folder tree for adding to new folders. Single entity shows membership;
 * multi-selection shows tree only.
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconFolder } from '@tabler/icons-react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { FolderTree } from '../../shared/ui/FolderTree';
import { TagChip } from '../../shared/ui/TagChip/TagChip';
import { folderPickerPortalAtom } from '../../state/portals';
import { selectionTargetAtom, selectionCountAtom } from '../../state/selection';
import { displayedInspectorEntityDataAtom } from '../../state/inspector';
import { folderNodesAtom, sidebarNodesAtom } from '../../state/sidebar';
import * as entityMutations from '../../controllers/entityMutations';
import styles from './FolderPickerPanel.module.css';

function hexToRgb(hex: string | null | undefined): [number, number, number] | undefined {
  if (!hex) return undefined;
  const h = hex.replace('#', '');
  if (h.length !== 6) return undefined;
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

export function FolderPickerPanel() {
  const portalState = useAtomValue(folderPickerPortalAtom);
  const setPortalState = useSetAtom(folderPickerPortalAtom);
  const target = useAtomValue(selectionTargetAtom);
  const selectionCount = useAtomValue(selectionCountAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const sidebarNodes = useAtomValue(sidebarNodesAtom);
  const entityData = useAtomValue(displayedInspectorEntityDataAtom);
  const open = portalState.open;
  const closeModal = useCallback(() => setPortalState({ open: false }), [setPortalState]);

  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const searchRef = useRef<HTMLInputElement>(null);

  const isSingle = selectionCount === 1;

  // Current folder memberships (single selection only)
  const currentFolders = useMemo(() => {
    if (!isSingle || !entityData?.folders) return [];
    return entityData.folders.map((f) => {
      const node = sidebarNodes.find((n) => n.id === `folder:${f.folder_id}`);
      return { folderId: f.folder_id, name: node?.name ?? f.name, color: node?.color ?? null };
    });
  }, [isSingle, entityData, sidebarNodes]);

  const memberOf = useMemo(
    () => new Set(currentFolders.map((f) => f.folderId)),
    [currentFolders],
  );

  useEffect(() => {
    if (open) {
      setQuery('');
      setSelected(new Set());
      setTimeout(() => searchRef.current?.focus(), 50);
    }
  }, [open]);

  const toggleFolder = useCallback((folderId: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(folderId)) next.delete(folderId);
      else next.add(folderId);
      return next;
    });
  }, []);

  const removeFromFolder = useCallback((folderId: number) => {
    if (!target) return;
    void entityMutations.updateTargetFolderMembership(target, folderId, 'remove');
  }, [target]);

  const applyFolders = useCallback(() => {
    if (!target || selected.size === 0) return;
    for (const folderId of selected) {
      void entityMutations.updateTargetFolderMembership(target, folderId, 'add');
    }
    closeModal();
  }, [target, selected, closeModal]);

  if (!open) return null;

  return (
    <GlassModal
      open={open}
      onClose={closeModal}
      title={isSingle ? 'Manage Folders' : `Add ${selectionCount} Items to Folder`}
      size="sm"
      flush
      footer={
        <>
          <button className={modalStyles.btn} onClick={closeModal} type="button">Cancel</button>
          <button
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            onClick={applyFolders}
            disabled={selected.size === 0}
            type="button"
          >
            Add{selected.size > 0 ? ` (${selected.size})` : ''}
          </button>
        </>
      }
    >
      {/* Current memberships */}
      {isSingle && (
        <div className={styles.membershipSection}>
          <span className={styles.sectionLabel}>Currently in</span>
          {currentFolders.length > 0 ? (
            <div className={styles.chipWrap}>
              {currentFolders.map((f) => (
                <TagChip
                  key={f.folderId}
                  namespace=""
                  subtag={f.name}
                  icon={<IconFolder size={12} />}
                  colorRgb={hexToRgb(f.color)}
                  onRemove={() => removeFromFolder(f.folderId)}
                />
              ))}
            </div>
          ) : (
            <span className={styles.noFolders}>No folders</span>
          )}
        </div>
      )}

      {/* Divider */}
      {isSingle && <div className={styles.divider} />}

      {/* Add section label */}
      <div className={styles.addSection}>
        <span className={styles.sectionLabel}>Add to folder</span>
      </div>

      {/* Search */}
      <div className={styles.searchArea}>
        <GlassInput
          ref={searchRef}
          search
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search folders..."
        />
      </div>

      {/* Folder tree */}
      <FolderTree
        nodes={folderNodes}
        selected={selected}
        onToggle={toggleFolder}
        search={query}
        memberOf={memberOf}
      />
    </GlassModal>
  );
}
