/**
 * FolderPickerPanel — OverlayShell portal for adding entities to folders.
 *
 * Same pattern as TagSelectPanel: search in header, tree in body,
 * kbd hints + apply in footer. Draggable, pinnable.
 * Supports Shift+click (range) and Ctrl/Cmd+click (toggle) multi-select.
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { OverlayShell } from '../../shared/ui/OverlayShell';
import { FolderTree, buildTree, flattenVisibleIds } from '../../shared/ui/FolderTree';
import { folderPickerPortalAtom } from '../../state/portals';
import { selectionTargetAtom } from '../../state/selection';
import { displayedInspectorEntityDataAtom } from '../../state/inspector';
import { folderNodesAtom } from '../../state/sidebar';
import * as entityMutations from '../../controllers/entityMutations';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';

export function FolderPickerPanel() {
  const portalState = useAtomValue(folderPickerPortalAtom);
  const setPortalState = useSetAtom(folderPickerPortalAtom);
  const target = useAtomValue(selectionTargetAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const entityData = useAtomValue(displayedInspectorEntityDataAtom);
  const open = portalState.open;
  const anchor = portalState.anchor ?? null;
  const close = useCallback(() => setPortalState({ open: false }), [setPortalState]);

  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [pinned, setPinned] = useState(false);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const searchRef = useRef<HTMLInputElement>(null);
  const lastClickedRef = useRef<number | null>(null);

  const memberOf = useMemo(() => {
    if (!entityData?.folders) return new Set<number>();
    return new Set(entityData.folders.map((f) => f.folder_id));
  }, [entityData]);

  // Build tree + flat visible IDs for range selection
  const tree = useMemo(() => buildTree(folderNodes), [folderNodes]);
  const searchLower = query.trim().toLowerCase();
  const flatIds = useMemo(
    () => flattenVisibleIds(tree, expanded, searchLower),
    [tree, expanded, searchLower],
  );

  // Sync expanded state when tree changes (auto-expand all)
  useEffect(() => {
    const ids = new Set<string>();
    (function walk(nodes) { for (const n of nodes) { ids.add(n.node.id); walk(n.children); } })(tree);
    setExpanded(ids);
  }, [tree]);

  useEffect(() => {
    if (open) {
      setQuery('');
      setSelected(new Set());
      lastClickedRef.current = null;
      setTimeout(() => searchRef.current?.focus(), 50);
    }
  }, [open]);

  const handleToggle = useCallback((folderId: number, event: React.MouseEvent) => {
    if (event.shiftKey && lastClickedRef.current != null) {
      const startIdx = flatIds.indexOf(lastClickedRef.current);
      const endIdx = flatIds.indexOf(folderId);
      if (startIdx !== -1 && endIdx !== -1) {
        const [lo, hi] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
        setSelected((prev) => {
          const next = (event.metaKey || event.ctrlKey) ? new Set(prev) : new Set<number>();
          for (let i = lo; i <= hi; i++) next.add(flatIds[i]);
          return next;
        });
      }
    } else if (event.metaKey || event.ctrlKey) {
      setSelected((prev) => {
        const next = new Set(prev);
        if (next.has(folderId)) next.delete(folderId); else next.add(folderId);
        return next;
      });
      lastClickedRef.current = folderId;
    } else {
      setSelected(new Set([folderId]));
      lastClickedRef.current = folderId;
    }
  }, [flatIds]);

  const applyFolders = useCallback(() => {
    if (!target || selected.size === 0) return;
    for (const folderId of selected) {
      void entityMutations.updateTargetFolderMembership(target, folderId, 'add');
    }
    close();
  }, [target, selected, close]);

  if (!open) return null;

  return (
    <OverlayShell
      open={open}
      onClose={close}
      width={340}
      height={480}
      pinned={pinned}
      onPinnedChange={setPinned}
      anchorPosition={anchor}
      header={
        <div className={shellStyles.searchRow} style={{ flex: 1 }}>
          <input
            ref={searchRef}
            className={shellStyles.searchInput}
            placeholder="Search folders..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      }
      footer={
        <>
          <span className={shellStyles.kbdHint}>
            <span className={shellStyles.kbd}>Esc</span> close
          </span>
          <div className={btnStyles.btnGroup}>
            {selected.size > 0 && (
              <button
                className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
                onClick={applyFolders}
                type="button"
              >
                Add ({selected.size})
              </button>
            )}
          </div>
        </>
      }
    >
      <FolderTree
        nodes={folderNodes}
        selected={selected}
        onToggle={handleToggle}
        search={query}
        memberOf={memberOf}
      />
    </OverlayShell>
  );
}
