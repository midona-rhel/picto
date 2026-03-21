import { useCallback, useEffect, useRef } from 'react';
import { api } from '#desktop/api';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import { useInlineRename } from '../../../shared/hooks/useInlineRename';
import { imageDrag } from '../../../shared/lib/imageDrag';
import { useImportActionStore } from '../../../state/importActionStore';
import { useFolderWatchActionStore } from '../../../state/folderWatchActionStore';
import { useNavigationStore } from '../../../state/navigationStore';
import type { SidebarNodeDto } from '../../../shared/types/sidebar';
import { TagSelectService } from '../../tags/components/tagSelectService';
import { type TreeNode, parseFolderId, getFolderAutoTags } from '../lib/folderTreeData';

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
    if (folderId != null) {
      try {
        const currentNode = folderNodes.find((n) => n.id === id);
        const oldName = currentNode?.name ?? '';
        if (oldName === newName) return;
        await api.folders.update({ folder_id: folderId, name: newName });
        registerUndoAction({
          label: 'Rename folder',
          undo: async () => {
            await api.folders.update({ folder_id: folderId, name: oldName });
          },
          redo: async () => {
            await api.folders.update({ folder_id: folderId, name: newName });
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
      let folder = await api.folders.create({ name: 'New Folder' });
      pendingRenameFolderId.current = folder.folder_id;
      startRename(`folder:${folder.folder_id}`, folder.name);
      registerUndoAction({
        label: 'Create folder',
        undo: async () => {
          await api.folders.delete(folder.folder_id);
        },
        redo: async () => {
          folder = await api.folders.create({ name: folder.name });
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
        files: await api.folders.getFiles(folderId),
      } : null;
      await api.folders.delete(folderId);
      if (snapshot) {
        let recreatedId: number | null = null;
        registerUndoAction({
          label: `Delete folder "${snapshot.name}"`,
          undo: async () => {
            const recreated = await api.folders.create({
              name: snapshot.name,
              parent_id: snapshot.parentId,
              icon: snapshot.icon ?? undefined,
              color: snapshot.color ?? undefined,
            });
            recreatedId = recreated.folder_id;
            if (snapshot.files.length > 0) {
              await api.folders.addFiles(recreated.folder_id, snapshot.files);
            }
          },
          redo: async () => {
            const id = recreatedId ?? folderId;
            await api.folders.delete(id);
          },
        });
      }
      if (activeFolderId === folderId) setActiveFolderId(null);
    } catch (e) { console.error('Delete failed:', e); }
  }, [activeFolderId, setActiveFolderId, nodeMap]);

  const handleBatchDelete = useCallback(async (ids: Set<string>) => {
    const folderIds = [...ids].map(parseFolderId).filter((id): id is number => id != null);
    if (folderIds.length === 0) return;
    // Capture folder info for undo
    const folderSnapshots = folderIds
      .map((id) => nodeMap.get(`folder-${id}`))
      .filter((n): n is NonNullable<typeof n> => n != null)
      .map((n) => ({ name: n.name, parentId: n.parent_id ? parseFolderId(n.parent_id) : null }));
    try {
      await Promise.all(folderIds.map((id) => api.folders.delete(id)));
      registerUndoAction({
        label: `Delete ${folderIds.length} folder${folderIds.length === 1 ? '' : 's'}`,
        undo: async () => {
          for (const snap of folderSnapshots) {
            await api.folders.create({ name: snap.name, parent_id: snap.parentId });
          }
        },
        redo: async () => {
          // Best-effort: folders may have been re-created with different IDs
        },
      });
      if (activeFolderId != null && folderIds.includes(activeFolderId)) setActiveFolderId(null);
    } catch (e) { console.error('Batch delete failed:', e); }
  }, [activeFolderId, setActiveFolderId, nodeMap]);

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
      await api.folders.reorder(moves);
      registerUndoAction({
        label: 'Sort folders',
        undo: async () => {
          await api.folders.reorder(previousMoves);
        },
        redo: async () => {
          await api.folders.reorder(moves);
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
      await api.folders.reorder(allMoves);
      registerUndoAction({
        label: 'Sort all folders',
        undo: async () => {
          await api.folders.reorder(previousMoves);
        },
        redo: async () => {
          await api.folders.reorder(allMoves);
        },
      });
    } catch (err) { console.error('Sort all failed:', err); }
  }, [folderNodes]);

  const createSiblingFolder = useCallback(async (node: TreeNode) => {
    const parentId = node.parent_id ? parseFolderId(node.parent_id) : null;
    let folder = await api.folders.create({ name: 'New Folder', parent_id: parentId ?? null });
    pendingRenameFolderId.current = folder.folder_id;
    startRename(`folder:${folder.folder_id}`, folder.name);
    registerUndoAction({
      label: 'Create folder',
      undo: async () => {
        await api.folders.delete(folder.folder_id);
      },
      redo: async () => {
        folder = await api.folders.create({ name: folder.name, parent_id: parentId ?? null });
      },
    });
  }, [startRename]);

  const createSubfolderForNode = useCallback(async (node: TreeNode, folderId: number) => {
    expandFolder(node.id);
    let sub = await api.folders.create({ name: 'New Folder', parent_id: folderId });
    pendingRenameFolderId.current = sub.folder_id;
    startRename(`folder:${sub.folder_id}`, sub.name);
    registerUndoAction({
      label: 'Create subfolder',
      undo: async () => {
        await api.folders.delete(sub.folder_id);
      },
      redo: async () => {
        sub = await api.folders.create({ name: sub.name, parent_id: folderId });
      },
    });
  }, [expandFolder, startRename]);

  const applyIconToFolders = useCallback(async (ids: number[], icon: string | null) => {
    const previous = ids.map((id) => {
      const targetNode = folderNodes.find((n) => parseFolderId(n.id) === id);
      return { id, icon: targetNode?.icon ?? null };
    });
    await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, icon: icon === null ? '' : (icon ?? undefined) })));
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder icons' : 'Change folder icon',
      undo: async () => {
        await Promise.all(previous.map((entry) => api.folders.update({ folder_id: entry.id, icon: entry.icon === null ? '' : (entry.icon ?? undefined) })));
      },
      redo: async () => {
        await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, icon: icon === null ? '' : (icon ?? undefined) })));
      },
    });
  }, [folderNodes]);

  const applyColorToFolders = useCallback(async (ids: number[], color: string | null) => {
    const previous = ids.map((id) => {
      const targetNode = folderNodes.find((n) => parseFolderId(n.id) === id);
      return { id, color: targetNode?.color ?? null };
    });
    await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, color: color === null ? '' : (color ?? undefined) })));
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder colors' : 'Change folder color',
      undo: async () => {
        await Promise.all(previous.map((entry) => api.folders.update({ folder_id: entry.id, color: entry.color === null ? '' : (entry.color ?? undefined) })));
      },
      redo: async () => {
        await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, color: color === null ? '' : (color ?? undefined) })));
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
        void api.folders.update({ folder_id: folderId, auto_tags: next }).then(() => {
          registerUndoAction({
            label: 'Update folder auto-tags',
            undo: async () => {
              await api.folders.update({ folder_id: folderId, auto_tags: prev });
            },
            redo: async () => {
              await api.folders.update({ folder_id: folderId, auto_tags: next });
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
      await api.folders.addFiles(folderId, hashes);
      registerUndoAction({
        label: `Add ${hashes.length} to folder`,
        undo: async () => {
          await api.folders.removeFiles(folderId, hashes);
        },
        redo: async () => {
          await api.folders.addFiles(folderId, hashes);
        },
      });
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
    await api.folders.clearWatchConfig(folderId);
  }, []);

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
    handleImportFolderHere,
    handleOpenFolderWatchDialog,
    handleClearFolderWatchConfig,
    // Re-export for context menu
    getFolderAutoTags,
  };
}
