import { useCallback, useEffect, useRef } from 'react';
import { foldersController } from '../../../controllers/foldersController';
import { useInlineRename } from '../../../shared/hooks/useInlineRename';
import { imageDrag } from '../../../shared/lib/imageDrag';
import { useImportActionStore } from '../../../state/importActionStore';
import { useFolderWatchActionStore } from '../../../state/folderWatchActionStore';
import { useNavigationStore } from '../../../state/navigationStore';
import type { SidebarNodeDto } from '../../../shared/types/sidebar';
import { TagSelectService } from '../../tags/components/tagSelectService';
import { type TreeNode, parseFolderId } from '../lib/folderTreeData';

interface UseFolderTreeActionsOptions {
  folderNodes: SidebarNodeDto[];
  nodeMap: Map<string, TreeNode>;
  activeFolderId: number | null;
  setActiveFolderId: (folderId: number | null) => void;
  expandFolder: (nodeId: string) => void;
  setCollapsedNodes: React.Dispatch<React.SetStateAction<Set<string>>>;
}

export function useFolderTreeActions({
  folderNodes,
  nodeMap,
  activeFolderId,
  setActiveFolderId,
  expandFolder,
  setCollapsedNodes,
}: UseFolderTreeActionsOptions) {
  const pendingRenameFolderId = useRef<number | null>(null);

  const handleRenameCommit = useCallback(async (id: string, newName: string) => {
    const folderId = parseFolderId(id);
    if (folderId == null) return;
    const currentNode = folderNodes.find((n) => n.id === id);
    const oldName = currentNode?.name ?? '';
    try {
      await foldersController.rename(folderId, newName, oldName);
    } catch (e) { console.error('Rename failed:', e); }
  }, [folderNodes]);

  const {
    renamingId, renameValue, startRename, setRenameValue,
    commitRename, renameInputRef, renameKeyHandler,
  } = useInlineRename(handleRenameCommit);

  useEffect(() => {
    if (pendingRenameFolderId.current == null) return;
    const pendingId = `folder:${pendingRenameFolderId.current}`;
    const found = folderNodes.find((n) => n.id === pendingId);
    if (found) {
      startRename(pendingId, found.name);
      pendingRenameFolderId.current = null;
      if (found.parent_id) {
        setCollapsedNodes((prev) => { const next = new Set(prev); next.delete(found.parent_id!); return next; });
      }
    }
  }, [folderNodes, startRename, setCollapsedNodes]);

  const handleCreate = useCallback(async () => {
    try {
      const folder = await foldersController.create({ name: 'New Folder' });
      pendingRenameFolderId.current = folder.folder_id;
      startRename(`folder:${folder.folder_id}`, folder.name);
    } catch (e) { console.error('Failed to create folder:', e); }
  }, [startRename]);

  const handleDelete = useCallback(async (nodeId: string) => {
    const folderId = parseFolderId(nodeId);
    if (folderId == null) return;
    try {
      const node = nodeMap.get(nodeId);
      const hasChildren = (node?.children.length ?? 0) > 0;
      const snapshot = !hasChildren && node ? {
        name: node.name,
        parentId: node.parent_id ? parseFolderId(node.parent_id) : null,
        icon: node.icon ?? null,
        color: node.color ?? null,
        files: await foldersController.getFiles(folderId),
      } : null;
      await foldersController.delete(folderId, snapshot);
      if (activeFolderId === folderId) setActiveFolderId(null);
    } catch (e) { console.error('Delete failed:', e); }
  }, [activeFolderId, setActiveFolderId, nodeMap]);

  const handleBatchDelete = useCallback(async (ids: Set<string>) => {
    const folderIds = [...ids].map(parseFolderId).filter((id): id is number => id != null);
    if (folderIds.length === 0) return;
    const snapshots = folderIds
      .map((id) => nodeMap.get(`folder-${id}`))
      .filter((n): n is NonNullable<typeof n> => n != null)
      .map((n) => ({ name: n.name, parentId: n.parent_id ? parseFolderId(n.parent_id) : null }));
    try {
      await foldersController.deleteBatch(folderIds, snapshots);
      if (activeFolderId != null && folderIds.includes(activeFolderId)) setActiveFolderId(null);
    } catch (e) { console.error('Batch delete failed:', e); }
  }, [activeFolderId, setActiveFolderId, nodeMap]);

  const handleSortFolders = useCallback(async (parentId: string | null, direction: 'asc' | 'desc') => {
    const siblings = folderNodes
      .filter((n) => n.parent_id === parentId)
      .sort((a, b) => direction === 'asc' ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name));
    const previousMoves: [number, number][] = siblings
      .map((n) => { const fid = parseFolderId(n.id); return fid != null ? [fid, n.sort_order ?? 0] as [number, number] : null; })
      .filter((m): m is [number, number] => m != null);
    const moves: [number, number][] = [];
    siblings.forEach((n, i) => { const fid = parseFolderId(n.id); if (fid != null) moves.push([fid, (i + 1) * 1000]); });
    try {
      await foldersController.reorder(moves, previousMoves);
    } catch (err) { console.error('Sort failed:', err); }
  }, [folderNodes]);

  const handleSortAllFolders = useCallback(async (direction: 'asc' | 'desc') => {
    const previousMoves: [number, number][] = folderNodes
      .map((n) => { const fid = parseFolderId(n.id); return fid != null ? [fid, n.sort_order ?? 0] as [number, number] : null; })
      .filter((m): m is [number, number] => m != null);
    const parentGroups = new Map<string | null, typeof folderNodes>();
    for (const n of folderNodes) { const key = n.parent_id ?? null; if (!parentGroups.has(key)) parentGroups.set(key, []); parentGroups.get(key)!.push(n); }
    const allMoves: [number, number][] = [];
    for (const [, siblings] of parentGroups) {
      siblings.sort((a, b) => direction === 'asc' ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name));
      siblings.forEach((n, i) => { const fid = parseFolderId(n.id); if (fid != null) allMoves.push([fid, (i + 1) * 1000]); });
    }
    try {
      await foldersController.reorder(allMoves, previousMoves);
    } catch (err) { console.error('Sort all failed:', err); }
  }, [folderNodes]);

  const createSiblingFolder = useCallback(async (node: TreeNode) => {
    const parentId = node.parent_id ? parseFolderId(node.parent_id) : null;
    try {
      const folder = await foldersController.create({ name: 'New Folder', parentId: parentId ?? null });
      pendingRenameFolderId.current = folder.folder_id;
      startRename(`folder:${folder.folder_id}`, folder.name);
    } catch (e) { console.error('Failed to create sibling folder:', e); }
  }, [startRename]);

  const createSubfolderForNode = useCallback(async (node: TreeNode, folderId: number) => {
    expandFolder(node.id);
    try {
      const sub = await foldersController.create({ name: 'New Folder', parentId: folderId });
      pendingRenameFolderId.current = sub.folder_id;
      startRename(`folder:${sub.folder_id}`, sub.name);
    } catch (e) { console.error('Failed to create subfolder:', e); }
  }, [expandFolder, startRename]);

  const applyIconToFolders = useCallback(async (ids: number[], icon: string | null) => {
    const previous = ids.map((id) => {
      const targetNode = folderNodes.find((n) => parseFolderId(n.id) === id);
      return { id, icon: targetNode?.icon ?? null };
    });
    await foldersController.applyIcon(ids, icon, previous);
  }, [folderNodes]);

  const applyColorToFolders = useCallback(async (ids: number[], color: string | null) => {
    const previous = ids.map((id) => {
      const targetNode = folderNodes.find((n) => parseFolderId(n.id) === id);
      return { id, color: targetNode?.color ?? null };
    });
    await foldersController.applyColor(ids, color, previous);
  }, [folderNodes]);

  const openFolderAutoTagsEditor = useCallback((folderId: number, folderName: string, currentTags: string[]) => {
    const original = [...currentTags];
    let draft = [...currentTags];
    TagSelectService.open({
      mode: 'modal',
      title: `Auto-Tags · ${folderName}`,
      anchorEl: null,
      selectedTags: draft,
      onToggle: (tag, added) => {
        draft = added ? [...draft, tag] : draft.filter((entry) => entry !== tag);
      },
      onClose: () => {
        const next = Array.from(new Set(draft)).sort();
        const prev = Array.from(new Set(original)).sort();
        if (JSON.stringify(next) === JSON.stringify(prev)) return;
        void foldersController.updateAutoTags(folderId, next, prev).catch((error) => {
          console.error('Failed to update folder auto-tags:', error);
        });
      },
    });
  }, []);

  const handleFilesDropOnFolder = useCallback(async (folderId: number, hashes: string[]) => {
    try {
      await foldersController.addFiles(folderId, hashes);
    } catch (err) { console.error('Failed to add files to folder:', err); }
  }, []);

  useEffect(() => {
    return imageDrag.onDrop(async ({ hashes, folderId }) => {
      handleFilesDropOnFolder(folderId, hashes);
    });
  }, [handleFilesDropOnFolder]);

  const handleImportFolderHere = useCallback((folderId: number, folderName: string) => {
    useNavigationStore.getState().navigateToFolder({ folder_id: folderId, name: folderName });
    useImportActionStore.getState().requestImportFolderDialog(folderId);
  }, []);

  const handleOpenFolderWatchDialog = useCallback((folderId: number) => {
    useFolderWatchActionStore.getState().requestOpen(folderId);
  }, []);

  const handleClearFolderWatchConfig = useCallback(async (folderId: number) => {
    await foldersController.clearWatchConfig(folderId);
  }, []);

  return {
    renamingId, renameValue, startRename, setRenameValue,
    commitRename, renameInputRef, renameKeyHandler,
    handleCreate,
    handleDelete,
    handleBatchDelete,
    handleSortFolders,
    handleSortAllFolders,
    createSiblingFolder,
    createSubfolderForNode,
    applyIconToFolders,
    applyColorToFolders,
    openFolderAutoTagsEditor,
    handleFilesDropOnFolder,
    handleImportFolderHere,
    handleOpenFolderWatchDialog,
    handleClearFolderWatchConfig,
  };
}
