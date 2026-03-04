import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { IconFolder, IconChevronRight } from '@tabler/icons-react';
import { mediaThumbnailUrl } from '../../lib/mediaUrl';
import { FolderController } from '../../controllers/folderController';
import { notifyError, notifySuccess } from '../../lib/notify';
import { useDomainStore } from '../../stores/domainStore';
import type { SidebarNodeDto } from '../../types/sidebar';
import { ContextMenu, ContextMenuEntry, useContextMenu } from '../ui/ContextMenu';
import {
  buildFolderMultiMenu,
  buildFolderSingleMenu,
  buildFolderSurfaceMenu,
} from '../sidebar/contextMenuRegistry';
import { DynamicIcon, DEFAULT_FOLDER_ICON } from '../smart-folders/iconRegistry';
import styles from './SubfolderGrid.module.css';

interface SubfolderGridProps {
  folderId: number;
  targetSize: number;
  totalImageCount: number;
  onOpenFolder: (folderId: number, name: string) => void;
  selectedSubfolderId: number | null;
  onSelectedSubfolderChange: (id: number | null) => void;
  paused?: boolean;
}

interface ChildFolder {
  folderId: number;
  name: string;
  icon: string | null;
  color: string | null;
  count: number;
  sortOrder: number;
}

function extractFolderId(node: SidebarNodeDto): number | null {
  const meta = node.meta as Record<string, unknown> | null | undefined;
  if (meta && typeof meta.folder_id === 'number') return meta.folder_id;
  const match = node.id.match(/^folder:(\d+)$/);
  return match ? parseInt(match[1], 10) : null;
}

function deriveChildFolders(folderNodes: SidebarNodeDto[], parentFolderId: number): ChildFolder[] {
  const parentNodeId = `folder:${parentFolderId}`;
  return folderNodes
    .filter(n => n.parent_id === parentNodeId)
    .map(n => ({
      folderId: extractFolderId(n) ?? 0,
      name: n.name,
      icon: n.icon ?? null,
      color: n.color ?? null,
      count: n.count ?? 0,
      sortOrder: n.sort_order ?? 0,
    }))
    .filter(f => f.folderId > 0)
    .sort((a, b) => a.sortOrder - b.sortOrder);
}

export function SubfolderGrid({ folderId, targetSize, totalImageCount, onOpenFolder, selectedSubfolderId, onSelectedSubfolderChange, paused = false }: SubfolderGridProps) {
  const folderNodes = useDomainStore(s => s.folderNodes);
  const [expanded, setExpanded] = useState(true);
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set());
  const lastClickedIndexRef = useRef<number | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);
  const dragStateRef = useRef<{ startX: number; startY: number } | null>(null);
  const dragPointerIdRef = useRef<number | null>(null);
  const [dragRect, setDragRect] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  const [coverHashes, setCoverHashes] = useState<Map<number, string | null>>(new Map());
  const coverHashesRef = useRef<Map<number, string | null>>(new Map());
  coverHashesRef.current = coverHashes;
  const contextMenu = useContextMenu();

  const childFolders = useMemo(
    () => deriveChildFolders(folderNodes, folderId),
    [folderNodes, folderId],
  );

  // Keep existing covers stable and fetch only missing covers.
  useEffect(() => {
    // Prune removed folders without clearing existing visible covers.
    setCoverHashes(prev => {
      const next = new Map<number, string | null>();
      for (const f of childFolders) {
        if (prev.has(f.folderId)) next.set(f.folderId, prev.get(f.folderId) ?? null);
      }
      return next;
    });

    if (paused || childFolders.length === 0) return;
    let cancelled = false;
    const missing = childFolders.filter(f => !coverHashesRef.current.has(f.folderId));
    if (missing.length === 0) return;

    Promise.all(
      missing.map(async (f) => {
        try {
          const hash = await FolderController.getFolderCoverHash(f.folderId);
          return [f.folderId, hash] as [number, string | null];
        } catch {
          return [f.folderId, null] as [number, string | null];
        }
      }),
    ).then(results => {
      if (cancelled) return;
      setCoverHashes(prev => {
        const next = new Map(prev);
        for (const [folderId, hash] of results) next.set(folderId, hash);
        return next;
      });
    });

    return () => { cancelled = true; };
  }, [childFolders, paused]);

  // Reset expand state when folder changes
  useEffect(() => {
    setExpanded(true);
    setSelectedIds(new Set());
    lastClickedIndexRef.current = null;
    setDragRect(null);
  }, [folderId]);

  const refreshSidebar = useCallback(() => {
  }, []);

  const createSubfolder = useCallback(async (parentId: number) => {
    try {
      await FolderController.createFolder({ name: 'New Folder', parentId });
      refreshSidebar();
      notifySuccess('Subfolder created', 'Folders');
    } catch (err) {
      notifyError(err, 'Create Subfolder Failed');
    }
  }, [refreshSidebar]);

  const renameFolder = useCallback(async (folder: ChildFolder) => {
    const next = window.prompt('Rename folder:', folder.name);
    if (next == null) return;
    const trimmed = next.trim();
    if (!trimmed || trimmed === folder.name) return;
    try {
      await FolderController.updateFolder({ folderId: folder.folderId, name: trimmed });
      refreshSidebar();
      notifySuccess('Folder renamed', 'Folders');
    } catch (err) {
      notifyError(err, 'Rename Folder Failed');
    }
  }, [refreshSidebar]);

  const applyColor = useCallback(async (folderIds: number[], color: string | null) => {
    try {
      await Promise.all(folderIds.map((id) => FolderController.updateFolder({ folderId: id, color })));
      refreshSidebar();
      notifySuccess(`Updated color for ${folderIds.length} folder${folderIds.length === 1 ? '' : 's'}`, 'Folders');
    } catch (err) {
      notifyError(err, 'Update Folder Color Failed');
    }
  }, [refreshSidebar]);

  const applyIcon = useCallback(async (folderIds: number[], icon: string | null) => {
    try {
      await Promise.all(folderIds.map((id) => FolderController.updateFolder({ folderId: id, icon })));
      refreshSidebar();
      notifySuccess(`Updated icon for ${folderIds.length} folder${folderIds.length === 1 ? '' : 's'}`, 'Folders');
    } catch (err) {
      notifyError(err, 'Update Folder Icon Failed');
    }
  }, [refreshSidebar]);

  const deleteFolders = useCallback(async (folderIds: number[]) => {
    try {
      await Promise.all(folderIds.map((id) => FolderController.deleteFolder(id)));
      refreshSidebar();
      setSelectedIds((prev) => {
        const next = new Set(prev);
        for (const id of folderIds) next.delete(id);
        return next;
      });
      if (selectedSubfolderId != null && folderIds.includes(selectedSubfolderId)) {
        onSelectedSubfolderChange(null);
      }
      notifySuccess(`Deleted ${folderIds.length} folder${folderIds.length === 1 ? '' : 's'}`, 'Folders');
    } catch (err) {
      notifyError(err, 'Delete Folder Failed');
    }
  }, [onSelectedSubfolderChange, refreshSidebar, selectedSubfolderId]);

  const handleTileClick = useCallback((e: React.MouseEvent, folder: ChildFolder, index: number) => {
    e.stopPropagation();
    if (e.metaKey || e.ctrlKey) {
      setSelectedIds((prev) => {
        const next = new Set(prev);
        if (next.has(folder.folderId)) next.delete(folder.folderId);
        else next.add(folder.folderId);
        return next;
      });
      lastClickedIndexRef.current = index;
      onSelectedSubfolderChange(null);
      return;
    }
    if (e.shiftKey && lastClickedIndexRef.current != null) {
      const lo = Math.min(lastClickedIndexRef.current, index);
      const hi = Math.max(lastClickedIndexRef.current, index);
      setSelectedIds((prev) => {
        const next = new Set(prev);
        for (let i = lo; i <= hi; i++) {
          const f = childFolders[i];
          if (f) next.add(f.folderId);
        }
        return next;
      });
      onSelectedSubfolderChange(null);
      return;
    }
    setSelectedIds(new Set());
    lastClickedIndexRef.current = index;
    onSelectedSubfolderChange(folder.folderId);
  }, [childFolders, onSelectedSubfolderChange]);

  const selectByDragRect = useCallback((x1: number, y1: number, x2: number, y2: number) => {
    const listEl = listRef.current;
    if (!listEl) return;
    const left = Math.min(x1, x2);
    const right = Math.max(x1, x2);
    const top = Math.min(y1, y2);
    const bottom = Math.max(y1, y2);
    const rect = { left, right, top, bottom };
    const hits = new Set<number>();
    const tiles = listEl.querySelectorAll<HTMLElement>('[data-subfolder-tile][data-folder-id]');
    tiles.forEach((tile) => {
      const tid = Number(tile.dataset.folderId);
      if (!Number.isFinite(tid)) return;
      const r = tile.getBoundingClientRect();
      const intersects = r.right > rect.left && r.left < rect.right && r.bottom > rect.top && r.top < rect.bottom;
      if (intersects) hits.add(tid);
    });
    setSelectedIds(hits);
    onSelectedSubfolderChange(null);
  }, [onSelectedSubfolderChange]);

  const handleListPointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest('[data-subfolder-tile]')) return;
    dragStateRef.current = { startX: e.clientX, startY: e.clientY };
    dragPointerIdRef.current = e.pointerId;
    setDragRect({ left: e.clientX, top: e.clientY, width: 0, height: 0 });
    setSelectedIds(new Set());
    onSelectedSubfolderChange(null);
  }, [onSelectedSubfolderChange]);

  useEffect(() => {
    if (!dragRect) return;

    const handleMove = (e: PointerEvent) => {
      const drag = dragStateRef.current;
      if (!drag) return;
      if (dragPointerIdRef.current != null && e.pointerId !== dragPointerIdRef.current) return;
      const left = Math.min(drag.startX, e.clientX);
      const top = Math.min(drag.startY, e.clientY);
      const width = Math.abs(e.clientX - drag.startX);
      const height = Math.abs(e.clientY - drag.startY);
      setDragRect({ left, top, width, height });
      selectByDragRect(drag.startX, drag.startY, e.clientX, e.clientY);
    };

    const finish = (e?: PointerEvent) => {
      if (e && dragPointerIdRef.current != null && e.pointerId !== dragPointerIdRef.current) return;
      dragStateRef.current = null;
      dragPointerIdRef.current = null;
      setDragRect(null);
    };

    const handleGlobalContext = () => finish();

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', finish);
    window.addEventListener('contextmenu', handleGlobalContext, true);
    return () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', finish);
      window.removeEventListener('contextmenu', handleGlobalContext, true);
    };
  }, [dragRect, selectByDragRect]);

  const handleTileContextMenu = useCallback((e: React.MouseEvent, folder: ChildFolder, index: number) => {
    e.stopPropagation();
    dragStateRef.current = null;
    dragPointerIdRef.current = null;
    setDragRect(null);
    const isMultiSelection = selectedIds.size > 1 && selectedIds.has(folder.folderId);
    if (!isMultiSelection) {
      setSelectedIds(new Set([folder.folderId]));
      lastClickedIndexRef.current = index;
    }
    const folderIds = isMultiSelection ? Array.from(selectedIds) : [folder.folderId];
    const items: ContextMenuEntry[] = isMultiSelection
      ? buildFolderMultiMenu({
        createSubfolder: () => createSubfolder(folderId),
        iconAndColor: {
          onIconChange: (icon) => applyIcon(folderIds, icon),
          onColorChange: (color) => applyColor(folderIds, color),
          iconLabel: 'Icon',
        },
        deleteFolders: () => deleteFolders(folderIds),
        deleteLabel: `Remove ${folderIds.length} Folders`,
      })
      : buildFolderSingleMenu({
        openFolder: () => onOpenFolder(folder.folderId, folder.name),
        createSubfolder: () => createSubfolder(folder.folderId),
        renameFolder: () => renameFolder(folder),
        iconAndColor: {
          iconValue: folder.icon,
          colorValue: folder.color,
          onIconChange: (icon) => applyIcon([folder.folderId], icon),
          onColorChange: (color) => applyColor([folder.folderId], color),
          iconLabel: 'Icon',
        },
        deleteFolder: () => deleteFolders([folder.folderId]),
        deleteLabel: 'Remove Folder',
      });
    contextMenu.open(e, items);
  }, [applyColor, applyIcon, contextMenu, createSubfolder, deleteFolders, folderId, onOpenFolder, renameFolder, selectedIds]);

  const handleGridContextMenu = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    if (target.closest('[data-subfolder-tile]')) return;
    dragStateRef.current = null;
    dragPointerIdRef.current = null;
    setDragRect(null);
    setSelectedIds(new Set());
    onSelectedSubfolderChange(null);
    contextMenu.open(e, buildFolderSurfaceMenu({
      createSubfolder: () => createSubfolder(folderId),
    }));
  }, [contextMenu, createSubfolder, folderId, onSelectedSubfolderChange]);

  const gridColumns = `repeat(auto-fill, minmax(${Math.max(80, targetSize)}px, 1fr))`;

  if (childFolders.length === 0) return null;

  return (
    <>
      <div className={styles.container} data-subfolder-grid>
      <div className={styles.sectionLabel} onClick={(e) => { e.stopPropagation(); setExpanded(v => !v); }}>
        <span className={`${styles.chevron} ${!expanded ? styles.chevronCollapsed : ''}`}>
          <IconChevronRight size={12} />
        </span>
        Folders ({childFolders.length})
      </div>

      {expanded && (
        <div
          ref={listRef}
          className={styles.list}
          style={{ gridTemplateColumns: gridColumns }}
          onContextMenu={handleGridContextMenu}
          onPointerDown={handleListPointerDown}
        >
          {childFolders.map((folder, index) => {
            const coverHash = coverHashes.get(folder.folderId);
            const isSelected = selectedSubfolderId === folder.folderId || selectedIds.has(folder.folderId);

            return (
              <div
                key={folder.folderId}
                data-subfolder-tile
                data-folder-id={folder.folderId}
                className={`${styles.tile} ${isSelected ? styles.tileSelected : ''}`}
                onClick={(e) => handleTileClick(e, folder, index)}
                onContextMenu={(e) => handleTileContextMenu(e, folder, index)}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  setSelectedIds(new Set());
                  onOpenFolder(folder.folderId, folder.name);
                }}
              >
                <div className={styles.thumbnail}>
                  <div className={styles.pic1}>
                    {coverHash ? (
                      <img
                        className={styles.coverImage}
                        src={mediaThumbnailUrl(coverHash)}
                        alt=""
                        loading="lazy"
                      />
                    ) : (
                      <div className={styles.folderPlaceholder}>
                        <IconFolder
                          size={Math.max(24, Math.round(targetSize * 0.25))}
                          color={folder.color ?? 'currentColor'}
                        />
                      </div>
                    )}
                  </div>
                  <div className={styles.pic2} />
                  <div className={styles.pic3} />
                </div>
                <div className={styles.nameRow}>
                  <DynamicIcon
                    name={folder.icon ?? DEFAULT_FOLDER_ICON}
                    size={14}
                    color={folder.color ?? 'var(--color-text-tertiary)'}
                  />
                  <div className={styles.name}>{folder.name}</div>
                </div>
                <div className={styles.metas}>
                  {folder.count} {folder.count === 1 ? 'item' : 'items'}
                </div>
              </div>
            );
          })}
          {dragRect && (
            <div
              className={styles.dragSelectRect}
              style={{
                left: dragRect.left,
                top: dragRect.top,
                width: dragRect.width,
                height: dragRect.height,
              }}
            />
          )}
        </div>
      )}

      {expanded && totalImageCount > 0 && (
        <div className={styles.contentLabel}>Content ({totalImageCount})</div>
      )}
      </div>

      {contextMenu.state && (
        <ContextMenu
          items={contextMenu.state.items}
          position={contextMenu.state.position}
          searchable={false}
          onClose={contextMenu.close}
        />
      )}
    </>
  );
}
