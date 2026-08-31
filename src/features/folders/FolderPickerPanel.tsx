/**
 * FolderPickerPanel — OverlayShell portal for adding entities to folders.
 *
 * Same pattern as TagSelectPanel: search in header, tree in body,
 * kbd hints in footer. Draggable, pinnable.
 * Supports Shift+click (range) and Ctrl/Cmd+click (toggle) multi-select.
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { OverlayShell, TabKeyHint } from '../../shared/ui/OverlayShell';
import { FolderTree, buildTree, flattenVisibleIds } from '../../shared/ui/FolderTree';
import {
  IconCheck,
  IconChecks,
  IconFolder,
  IconFolderPlus,
  IconFolders,
  IconHistory,
  IconSearch,
} from '@tabler/icons-react';
import { folderPickerPortalAtom } from '../../state/portals';
import { selectionTargetAtom } from '../../state/selection';
import { displayedInspectorItemDetailsAtom } from '../../state/inspector';
import { folderNodesAtom } from '../../state/sidebar';
import * as entityMutations from '../../controllers/entityMutations';
import { recordRecentFolderUse, useRecentFolders } from '../../shared/hooks/useRecentFolders';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal/GlassModal';
import { foldersController } from '../../controllers/foldersController';
import { FilterLogicTabs } from '../../shared/ui/FilterLogicTabs';
import type { FilterMatchMode } from '../../shared/types/generated/application/FilterMatchMode';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import styles from './FolderPickerPanel.module.css';
import { t } from '../../i18n';

type FolderView = 'all' | 'recent' | 'selected';

export function FolderPickerPanel() {
  const portalState = useAtomValue(folderPickerPortalAtom);
  const setPortalState = useSetAtom(folderPickerPortalAtom);
  const selectionTarget = useAtomValue(selectionTargetAtom);
  const target = portalState.target ?? selectionTarget;
  const folderNodes = useAtomValue(folderNodesAtom);
  const entityData = useAtomValue(displayedInspectorItemDetailsAtom);
  const open = portalState.open;
  const anchor = portalState.anchor ?? null;
  const filterSelection = portalState.onApplyFolderFilter != null;
  const parentSelection = portalState.onApplyFolderParent != null;
  const immediateSelection = !filterSelection && !parentSelection;
  const close = useCallback(() => setPortalState({ open: false }), [setPortalState]);

  const [query, setQuery] = useState('');
  const [view, setView] = useState<FolderView>('all');
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [excluded, setExcluded] = useState<Set<number>>(new Set());
  const [rootSelected, setRootSelected] = useState(false);
  const [matchMode, setMatchMode] = useState<FilterMatchMode>('any');
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const searchRef = useRef<HTMLInputElement>(null);
  const lastClickedRef = useRef<number | null>(null);
  const contextMenu = useContextMenu();
  const [createTarget, setCreateTarget] = useState<{ parentId: number | null; title: string } | null>(null);
  const [folderName, setFolderName] = useState('');
  const [recentFolderIds] = useRecentFolders(20);

  // Build tree + flat visible IDs for range selection
  const availableFolders = useMemo(() => {
    if (!portalState.availableFolderIds) return folderNodes;
    const available = new Set(portalState.availableFolderIds);
    return folderNodes.filter((node) => available.has(Number(node.id.slice(7))));
  }, [folderNodes, portalState.availableFolderIds]);
  const tree = useMemo(() => buildTree(availableFolders), [availableFolders]);
  const searchLower = query.trim().toLowerCase();
  const flatIds = useMemo(
    () => flattenVisibleIds(tree, expanded, searchLower),
    [tree, expanded, searchLower],
  );

  const displayedFolders = useMemo(() => {
    if (view === 'all') return availableFolders;
    const byId = new Map(availableFolders.map((node) => [Number(node.id.slice(7)), node]));
    const ids = view === 'recent'
      ? recentFolderIds
      : [...selected];
    return ids.flatMap((folderId, index) => {
      const node = byId.get(folderId);
      return node ? [{ ...node, parent_id: 'section:folders', sort_order: index }] : [];
    });
  }, [availableFolders, recentFolderIds, selected, view]);

  // Sync expanded state when tree changes (auto-expand all)
  useEffect(() => {
    const ids = new Set<string>();
    (function walk(nodes) { for (const n of nodes) { ids.add(n.node.id); walk(n.children); } })(tree);
    setExpanded(ids);
  }, [tree]);

  useEffect(() => {
    if (open) {
      setQuery('');
      setView('all');
      setSelected(new Set(portalState.selectedFolderIds ?? (immediateSelection ? entityData?.folder_ids ?? [] : [])));
      setRootSelected(parentSelection && (portalState.selectedFolderIds?.length ?? 0) === 0);
      setExcluded(filterSelection ? new Set(portalState.excludedFolderIds ?? []) : new Set());
      setMatchMode(portalState.filterMatchMode ?? 'any');
      lastClickedRef.current = null;
      setTimeout(() => searchRef.current?.focus(), 50);
    }
  }, [open, entityData?.folder_ids, filterSelection, immediateSelection, parentSelection, portalState.selectedFolderIds, portalState.excludedFolderIds, portalState.filterMatchMode]);

  const commitImmediateSelection = useCallback((next: Set<number>, folderId: number, adding: boolean) => {
    if (portalState.onApplyFolders) {
      portalState.onApplyFolders([...next]);
      if (adding) recordRecentFolderUse([folderId]);
    } else if (target) {
      void entityMutations.updateTargetFolderMembership(target, folderId, adding ? 'add' : 'remove');
    }
  }, [portalState.onApplyFolders, target]);

  const commitFilterSelection = useCallback((nextSelected: Set<number>, nextExcluded: Set<number>, mode = matchMode) => {
    portalState.onApplyFolderFilter?.([...nextSelected], [...nextExcluded], mode);
  }, [matchMode, portalState.onApplyFolderFilter]);

  const handleToggle = useCallback((folderId: number, event: React.MouseEvent) => {
    if (parentSelection) {
      setSelected(new Set([folderId]));
      setRootSelected(false);
      return;
    }
    if (immediateSelection) {
      const next = new Set(selected);
      const removing = next.delete(folderId);
      if (!removing) next.add(folderId);
      setSelected(next);
      commitImmediateSelection(next, folderId, !removing);
      return;
    }
    if (filterSelection) {
      const nextSelected = new Set(selected);
      const nextExcluded = new Set(excluded);
      nextExcluded.delete(folderId);
      if (event.shiftKey && lastClickedRef.current != null) {
        const startIdx = flatIds.indexOf(lastClickedRef.current);
        const endIdx = flatIds.indexOf(folderId);
        if (startIdx !== -1 && endIdx !== -1) {
          if (!event.metaKey && !event.ctrlKey) nextSelected.clear();
          const [lo, hi] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
          for (let i = lo; i <= hi; i++) nextSelected.add(flatIds[i]);
        }
      } else if (!nextSelected.delete(folderId)) {
        nextSelected.add(folderId);
      }
      setSelected(nextSelected);
      setExcluded(nextExcluded);
      lastClickedRef.current = folderId;
      recordRecentFolderUse([folderId]);
      commitFilterSelection(nextSelected, nextExcluded);
      return;
    }
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
  }, [commitFilterSelection, commitImmediateSelection, excluded, filterSelection, flatIds, immediateSelection, parentSelection, selected]);

  const handleExclude = useCallback((folderId: number) => {
    if (!filterSelection) return;
    const nextSelected = new Set(selected);
    const nextExcluded = new Set(excluded);
    nextSelected.delete(folderId);
    if (!nextExcluded.delete(folderId)) nextExcluded.add(folderId);
    setSelected(nextSelected);
    setExcluded(nextExcluded);
    recordRecentFolderUse([folderId]);
    commitFilterSelection(nextSelected, nextExcluded);
  }, [commitFilterSelection, excluded, filterSelection, selected]);

  const openFolderContextMenu = useCallback((folderId: number, event: React.MouseEvent) => {
    const node = folderNodes.find((candidate) => candidate.id === `folder:${folderId}`);
    const parentId = node?.parent_id?.startsWith('folder:')
      ? Number(node.parent_id.slice(7))
      : null;
    const openCreate = (targetParentId: number | null, title: string) => {
      setFolderName('');
      setCreateTarget({ parentId: targetParentId, title });
    };
    contextMenu.open(event, [
      {
        label: t("New Subfolder"),
        icon: <IconFolderPlus size={14} />,
        action: () => openCreate(folderId, 'New Subfolder'),
      },
      {
        label: t("New Sibling Folder"),
        icon: <IconFolderPlus size={14} />,
        action: () => openCreate(parentId, 'New Sibling Folder'),
      },
    ], { showSearch: false });
  }, [contextMenu, folderNodes]);

  const createNamedFolder = useCallback(async () => {
    const name = folderName.trim();
    if (!name || !createTarget) return;
    const nodeId = await foldersController.create(name, createTarget.parentId);
    const folderId = Number(nodeId.slice(7));
    const next = immediateSelection ? new Set([...selected, folderId]) : new Set([folderId]);
    setSelected(next);
    if (immediateSelection) commitImmediateSelection(next, folderId, true);
    setRootSelected(false);
    setCreateTarget(null);
  }, [commitImmediateSelection, createTarget, folderName, immediateSelection, selected]);

  const moveFolder = useCallback(() => {
    portalState.onApplyFolderParent?.(rootSelected ? null : [...selected][0] ?? null);
    close();
  }, [close, portalState, rootSelected, selected]);

  const changeMatchMode = useCallback((mode: FilterMatchMode) => {
    setMatchMode(mode);
    commitFilterSelection(selected, excluded, mode);
  }, [commitFilterSelection, excluded, selected]);

  if (!open) return null;

  return (
    <OverlayShell
      open={open}
      onClose={close}
      width={360}
      height={480}
      anchorPosition={anchor}
      anchorPlacement={portalState.anchorPlacement}
      header={
        <>
          <div className={shellStyles.searchRow} style={{ flex: 1 }}>
            <IconSearch size={14} className={shellStyles.searchIcon} />
            <input
              ref={searchRef}
              className={shellStyles.searchInput}
              placeholder={t("Search...")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <div className={shellStyles.viewTabs} role="group" aria-label={t("Folder view")}>
            <KbdTooltip label={t("All folders")}><button
              type="button"
              className={`${shellStyles.viewTab} ${view === 'all' ? shellStyles.viewTabActive : ''}`}
              aria-label={t("All folders")}
              onClick={() => setView('all')}
            ><IconFolders size={14} /></button></KbdTooltip>
            <KbdTooltip label={t("Recent folders")}><button
              type="button"
              className={`${shellStyles.viewTab} ${view === 'recent' ? shellStyles.viewTabActive : ''}`}
              aria-label={t("Recent folders")}
              onClick={() => setView('recent')}
            ><IconHistory size={14} /></button></KbdTooltip>
            <KbdTooltip label={t("Selected folders")}><button
              type="button"
              className={`${shellStyles.viewTab} ${view === 'selected' ? shellStyles.viewTabActive : ''}`}
              aria-label={t("Selected folders")}
              onClick={() => setView('selected')}
            ><IconChecks size={14} /></button></KbdTooltip>
          </div>
          {filterSelection ? <FilterLogicTabs value={matchMode} onChange={changeMatchMode} /> : null}
        </>
      }
      footer={
        <>
          <div className={styles.footerHints}>
            <span className={shellStyles.kbdHint}>{t("Switch ")}<TabKeyHint /></span>
            <span className={shellStyles.kbdHint}>{t("Move ")}<span className={styles.keyPair}><span className={shellStyles.kbd}>↑</span><span className={shellStyles.kbd}>↓</span></span></span>
            <span className={shellStyles.kbdHint}>{t("Select ")}<span className={shellStyles.kbd}>↵</span></span>
          </div>
          <div className={`${btnStyles.btnGroup} ${styles.footerEnd}`}>
            {parentSelection && (
              <button
                className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
                onClick={moveFolder}
                type="button"
              >
                {t("Move")}</button>
            )}
            {!parentSelection && <span className={shellStyles.kbdHint}>{t("Close ")}<span className={shellStyles.kbd}>{t("Esc")}</span></span>}
          </div>
        </>
      }
    >
      {parentSelection && (!query || 'Library'.toLowerCase().includes(query.toLowerCase())) ? (
        <div
          className={`${shellStyles.checkRow} ${rootSelected ? shellStyles.checkRowActive : ''}`}
          onClick={() => { setSelected(new Set()); setRootSelected(true); }}
        >
          <div className={`${shellStyles.checkBox} ${rootSelected ? shellStyles.checkBoxChecked : ''}`}>
            {rootSelected ? <IconCheck size={10} /> : null}
          </div>
          <IconFolder size={14} />
          <span className={shellStyles.checkLabel}>{t("Library")}</span>
        </div>
      ) : null}
      <FolderTree
        nodes={displayedFolders}
        selected={selected}
        onToggle={handleToggle}
        excluded={filterSelection ? excluded : undefined}
        filterSelection={filterSelection}
        onExclude={filterSelection ? handleExclude : undefined}
        onContextMenu={filterSelection ? undefined : openFolderContextMenu}
        search={query}
      />
      {contextMenu.state ? (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          showSearch={contextMenu.state.showSearch}
          onClose={contextMenu.close}
        />
      ) : null}
      <GlassModal
        open={createTarget != null}
        onClose={() => setCreateTarget(null)}
        title={createTarget?.title ?? 'New Folder'}
        size="sm"
        footer={(
          <>
            <button type="button" className={modalStyles.btn} onClick={() => setCreateTarget(null)}>{t("Cancel")}</button>
            <button data-modal-primary="true" type="submit" form="folder-picker-create-form" className={`${modalStyles.btn} ${modalStyles.btnPrimary}`} disabled={!folderName.trim()}>{t("Create")}</button>
          </>
        )}
      >
        <form id="folder-picker-create-form" onSubmit={(event) => { event.preventDefault(); void createNamedFolder(); }}>
          <input
            autoFocus
            className={modalStyles.textInput}
            aria-label={t("Folder name")}
            placeholder={t("Folder name")}
            value={folderName}
            onChange={(event) => setFolderName(event.target.value)}
          />
        </form>
      </GlassModal>
    </OverlayShell>
  );
}
