/**
 * FolderPickerPanel — centered modal for adding entities to folders.
 *
 * Shows the full folder tree with search. Folders the entity is already in
 * are shown as non-toggleable "Member" rows for context. New folders can be
 * checked and applied with the Add button.
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { GlassInput } from '../../shared/ui/GlassInput';
import { FolderTree } from '../../shared/ui/FolderTree';
import { folderPickerPortalAtom } from '../../state/portals';
import { selectionTargetAtom } from '../../state/selection';
import { displayedInspectorEntityDataAtom } from '../../state/inspector';
import { folderNodesAtom } from '../../state/sidebar';
import * as entityMutations from '../../controllers/entityMutations';

export function FolderPickerPanel() {
  const portalState = useAtomValue(folderPickerPortalAtom);
  const setPortalState = useSetAtom(folderPickerPortalAtom);
  const target = useAtomValue(selectionTargetAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const entityData = useAtomValue(displayedInspectorEntityDataAtom);
  const open = portalState.open;
  const closeModal = useCallback(() => setPortalState({ open: false }), [setPortalState]);

  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const searchRef = useRef<HTMLInputElement>(null);

  // Folder IDs the entity is already a member of
  const memberOf = useMemo(() => {
    if (!entityData?.folders) return new Set<number>();
    return new Set(entityData.folders.map((f) => f.folder_id));
  }, [entityData]);

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
      title="Add to Folder"
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
      <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--color-border-secondary)' }}>
        <GlassInput
          ref={searchRef}
          search
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search folders..."
        />
      </div>
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
