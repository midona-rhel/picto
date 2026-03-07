import { useCallback, useEffect, useRef } from 'react';
import { FolderController } from '../../../controllers/folderController';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import { notifyWarning } from '../../../shared/lib/notify';
import { useInlineRename } from '../../../shared/hooks/useInlineRename';
import { imageDrag } from '../../../shared/lib/imageDrag';
import type { SidebarNodeDto } from '../../../shared/types/sidebar';
import { TagSelectService } from '../../tags/components/tagSelectService';
import { type TreeNode, parseFolderId, getFolderAutoTags } from '../lib/folderTreeData';

interface UseFolderTreeActionsOptions {
  folderNodes: SidebarNodeDto[];
  nodeMap: Map<string, TreeNode>;
  activeFolder: { folder_id: number; name: string } | null;
  setActiveFolder: (folder: { folder_id: number; name: string } | null) => void;
  expandFolder: (nodeId: string) => void;
  setCollapsedNodes: React.Dispatch<React.SetStateAction<Set<string>>>;
}

export function useFolderTreeActions({
  folderNodes,
  nodeMap,
  activeFolder,
  setActiveFolder,
  expandFolder,
  setCollapsedNodes,
}: UseFolderTreeActionsOptions) {
  const pendingRenameFolderId = useRef<number | null>(null);

  const handleRenameCommit = useCallback(async (id: string, newName: string) => {
    const folderId = parseFolderId(id);
    if (folderId != null) {
      try {
        const currentNode = folderNodes.find((n) => n.id === id);
        const oldName = currentNode?.name ?? '';
        if (oldName === newName) return;
        await FolderController.updateFolder({ folderId, name: newName });
        registerUndoAction({
          label: 'Rename folder',
          undo: async () => {
            await FolderController.updateFolder({ folderId, name: oldName });
          },
          redo: async () => {
            await FolderController.updateFolder({ folderId, name: newName });
          },
        });
      } catch (e) { console.error('Rename failed:', e); }
    }
  }, [folderNodes]);

  const {
    renamingId, renameValue, startRename, setRenameValue,
    commitRename, renameInputRef, renameKeyHandler,
  } = useInlineRename(handleRenameCommit);

  // Auto-start rename when a newly created folder appears in the store
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
      let folder = await FolderController.createFolder({ name: 'New Folder' });
      pendingRenameFolderId.current = folder.folder_id;
      startRename(`folder:${folder.folder_id}`, folder.name);
      registerUndoAction({
        label: 'Create folder',
        undo: async () => {
          await FolderController.deleteFolder(folder.folder_id);
        },
        redo: async () => {
          folder = await FolderController.createFolder({ name: folder.name });
        },
      });
    } catch (e) {
      console.error('Failed to create folder:', e);
    }
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
        files: await FolderController.getFolderFiles(folderId),
      } : null;
      await FolderController.deleteFolder(folderId);
      if (snapshot) {
        let recreatedId: number | null = null;
        registerUndoAction({
          label: `Delete folder "${snapshot.name}"`,
          undo: async () => {
            const recreated = await FolderController.createFolder({
              name: snapshot.name,
              parentId: snapshot.parentId,
              icon: snapshot.icon,
              color: snapshot.color,
            });
            recreatedId = recreated.folder_id;
            if (snapshot.files.length > 0) {
              await FolderController.addFilesToFolderBatch(recreated.folder_id, snapshot.files);
            }
          },
          redo: async () => {
            const id = recreatedId ?? folderId;
            await FolderController.deleteFolder(id);
          },
        });
      } else {
        notifyWarning('Undo for deleting folders with subfolders is not supported yet.', 'Limited Undo');
      }
      if (activeFolder?.folder_id === folderId) setActiveFolder(null);
    } catch (e) { console.error('Delete failed:', e); }
  }, [activeFolder, setActiveFolder, nodeMap]);

  const handleBatchDelete = useCallback(async (ids: Set<string>) => {
    const folderIds = [...ids].map(parseFolderId).filter((id): id is number => id != null);
    if (folderIds.length === 0) return;
    try {
      await Promise.all(folderIds.map((id) => FolderController.deleteFolder(id)));
      notifyWarning('Batch folder delete cannot be undone yet.', 'Limited Undo');
      if (activeFolder && folderIds.includes(activeFolder.folder_id)) setActiveFolder(null);
    } catch (e) { console.error('Batch delete failed:', e); }
  }, [activeFolder, setActiveFolder]);

  const handleSortFolders = useCallback(async (parentId: string | null, direction: 'asc' | 'desc') => {
    const siblings = folderNodes
      .filter((n) => n.parent_id === parentId)
      .sort((a, b) => direction === 'asc' ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name));
    const previousMoves: [number, number][] = siblings
      .map((n) => {
        const fid = parseFolderId(n.id);
        if (fid == null) return null;
        return [fid, n.sort_order ?? 0] as [number, number];
      })
      .filter((m): m is [number, number] => m != null);
    const moves: [number, number][] = [];
    siblings.forEach((n, i) => {
      const fid = parseFolderId(n.id);
      if (fid != null) moves.push([fid, (i + 1) * 1000]);
    });
    try {
      await FolderController.reorderFolders(moves);
      registerUndoAction({
        label: 'Sort folders',
        undo: async () => {
          await FolderController.reorderFolders(previousMoves);
        },
        redo: async () => {
          await FolderController.reorderFolders(moves);
        },
      });
    } catch (err) { console.error('Sort failed:', err); }
  }, [folderNodes]);

  const handleSortAllFolders = useCallback(async (direction: 'asc' | 'desc') => {
    const previousMoves: [number, number][] = folderNodes
      .map((n) => {
        const fid = parseFolderId(n.id);
        if (fid == null) return null;
        return [fid, n.sort_order ?? 0] as [number, number];
      })
      .filter((m): m is [number, number] => m != null);
    const parentGroups = new Map<string | null, typeof folderNodes>();
    for (const n of folderNodes) {
      const key = n.parent_id ?? null;
      if (!parentGroups.has(key)) parentGroups.set(key, []);
      parentGroups.get(key)!.push(n);
    }
    const allMoves: [number, number][] = [];
    for (const [, siblings] of parentGroups) {
      siblings.sort((a, b) => direction === 'asc' ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name));
      siblings.forEach((n, i) => {
        const fid = parseFolderId(n.id);
        if (fid != null) allMoves.push([fid, (i + 1) * 1000]);
      });
    }
    try {
      await FolderController.reorderFolders(allMoves);
      registerUndoAction({
        label: 'Sort all folders',
        undo: async () => {
          await FolderController.reorderFolders(previousMoves);
        },
        redo: async () => {
          await FolderController.reorderFolders(allMoves);
        },
      });
    } catch (err) { console.error('Sort all failed:', err); }
  }, [folderNodes]);

  const createSiblingFolder = useCallback(async (node: TreeNode) => {
    const parentId = node.parent_id ? parseFolderId(node.parent_id) : null;
    let folder = await FolderController.createFolder({ name: 'New Folder', parentId });
    pendingRenameFolderId.current = folder.folder_id;
    startRename(`folder:${folder.folder_id}`, folder.name);
    registerUndoAction({
      label: 'Create folder',
      undo: async () => {
        await FolderController.deleteFolder(folder.folder_id);
      },
      redo: async () => {
        folder = await FolderController.createFolder({ name: folder.name, parentId });
      },
    });
  }, [startRename]);

  const createSubfolderForNode = useCallback(async (node: TreeNode, folderId: number) => {
    expandFolder(node.id);
    let sub = await FolderController.createFolder({ name: 'New Folder', parentId: folderId });
    pendingRenameFolderId.current = sub.folder_id;
    startRename(`folder:${sub.folder_id}`, sub.name);
    registerUndoAction({
      label: 'Create subfolder',
      undo: async () => {
        await FolderController.deleteFolder(sub.folder_id);
      },
      redo: async () => {
        sub = await FolderController.createFolder({ name: sub.name, parentId: folderId });
      },
    });
  }, [expandFolder, startRename]);

  const applyIconToFolders = useCallback(async (ids: number[], icon: string | null) => {
    const previous = ids.map((id) => {
      const targetNode = folderNodes.find((n) => parseFolderId(n.id) === id);
      return { id, icon: targetNode?.icon ?? null };
    });
    await Promise.all(ids.map((id) => FolderController.updateFolder({ folderId: id, icon })));
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder icons' : 'Change folder icon',
      undo: async () => {
        await Promise.all(previous.map((entry) => FolderController.updateFolder({ folderId: entry.id, icon: entry.icon })));
      },
      redo: async () => {
        await Promise.all(ids.map((id) => FolderController.updateFolder({ folderId: id, icon })));
      },
    });
  }, [folderNodes]);

  const applyColorToFolders = useCallback(async (ids: number[], color: string | null) => {
    const previous = ids.map((id) => {
      const targetNode = folderNodes.find((n) => parseFolderId(n.id) === id);
      return { id, color: targetNode?.color ?? null };
    });
    await Promise.all(ids.map((id) => FolderController.updateFolder({ folderId: id, color })));
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder colors' : 'Change folder color',
      undo: async () => {
        await Promise.all(previous.map((entry) => FolderController.updateFolder({ folderId: entry.id, color: entry.color })));
      },
      redo: async () => {
        await Promise.all(ids.map((id) => FolderController.updateFolder({ folderId: id, color })));
      },
    });
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
        void FolderController.updateFolder({ folderId, autoTags: next }).then(() => {
          registerUndoAction({
            label: 'Update folder auto-tags',
            undo: async () => {
              await FolderController.updateFolder({ folderId, autoTags: prev });
            },
            redo: async () => {
              await FolderController.updateFolder({ folderId, autoTags: next });
            },
          });
        }).catch((error) => {
          console.error('Failed to update folder auto-tags:', error);
        });
      },
    });
  }, []);

  const handleFilesDropOnFolder = useCallback(async (folderId: number, hashes: string[]) => {
    try {
      await FolderController.addFilesToFolderBatch(folderId, hashes);
      registerUndoAction({
        label: `Add ${hashes.length} to folder`,
        undo: async () => {
          await FolderController.removeFilesFromFolderBatch(folderId, hashes);
        },
        redo: async () => {
          await FolderController.addFilesToFolderBatch(folderId, hashes);
        },
      });
    } catch (err) { console.error('Failed to add files to folder:', err); }
  }, []);

  useEffect(() => {
    return imageDrag.onDrop(async ({ hashes, folderId }) => {
      handleFilesDropOnFolder(folderId, hashes);
    });
  }, [handleFilesDropOnFolder]);

  return {
    // Rename state
    renamingId,
    renameValue,
    startRename,
    setRenameValue,
    commitRename,
    renameInputRef,
    renameKeyHandler,
    // CRUD actions
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
    // Re-export for context menu
    getFolderAutoTags,
  };
}
