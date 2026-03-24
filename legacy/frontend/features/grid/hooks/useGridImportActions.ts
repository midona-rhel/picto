import { useCallback, useEffect, useState } from 'react';
import { notifyError } from '../../../shared/lib/notify';
import { getCurrentWebview, open } from '#desktop/api';
import type { DragDropPayload } from '../../../shared/types/api';
import { importController } from '../../../controllers/importController';
import { foldersController } from '../../../controllers/foldersController';
import { useImportActionStore } from '../../../state-legacy/importActionStore';
import { useTaskStore } from '../../../state-legacy/taskStore';
import { imageDrag } from '../../../shared/lib/imageDrag';

export function useGridImportActions(args: {
  folderIdRef: React.MutableRefObject<number | null | undefined>;
  setDragOver: (over: boolean) => void;
}) {
  const { folderIdRef, setDragOver } = args;
  const [folderImportDialog, setFolderImportDialog] = useState<{
    path: string;
    preserveStructure: boolean;
    targetFolderId: number | null;
  } | null>(null);

  const importRequestToken = useImportActionStore((s) => s.requestToken);
  const importHandledToken = useImportActionStore((s) => s.handledToken);
  const importRequestKind = useImportActionStore((s) => s.requestKind);
  const importTargetFolderId = useImportActionStore((s) => s.targetFolderId);
  const markImportHandled = useImportActionStore((s) => s.markHandled);

  const importPaths = useCallback(async (paths: string[]) => {
    if (paths.length === 0) return;
    const ts = useTaskStore.getState();
    const currentFolderId = folderIdRef.current;
    let imported = 0;
    let skipped = 0;
    let errors = 0;
    ts.startFamily('import', 'Importing files');

    for (let index = 0; index < paths.length; index += 1) {
      const result = await importController.importFiles([paths[index]]);
      const importedHashes = result.imported.map((file) => file.hash);
      if (currentFolderId != null && importedHashes.length > 0) {
        await foldersController.addFiles(currentFolderId, importedHashes);
      }
      imported += result.imported.length;
      skipped += result.skipped.length;
      errors += result.errors.length;
      ts.updateFamilyProgress('import', {
        done: index + 1,
        total: paths.length,
        statusText: `${imported} imported, ${skipped} skipped`,
        imported, skipped, errors,
      });
    }

    ts.finishFamily('import');
  }, [folderIdRef]);

  const handleImport = useCallback(async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [{
          name: 'Images',
          extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'tiff', 'svg', 'mp4', 'webm', 'mov', 'mkv', 'avi'],
        }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      await importPaths(paths);
    } catch (err) {
      useTaskStore.getState().failFamily('import');
      notifyError(err, 'Import Failed');
    }
  }, [importPaths]);

  const handleImportFolderRequest = useCallback(async (targetFolderId?: number | null) => {
    const selected = await open({
      properties: ['openDirectory'],
      message: 'Select a folder to import',
    });
    const pickedPath = Array.isArray(selected) ? selected[0] : selected;
    if (!pickedPath) return;
    setFolderImportDialog({
      path: pickedPath,
      preserveStructure: true,
      targetFolderId: targetFolderId ?? null,
    });
  }, []);

  const handleConfirmImportFolder = useCallback(async () => {
    const pendingImport = folderImportDialog;
    if (!pendingImport) return;
    setFolderImportDialog(null);
    try {
      useTaskStore.getState().startFamily('import', 'Adding folder');
      await importController.importFolder(
        pendingImport.path,
        pendingImport.preserveStructure,
        pendingImport.targetFolderId ?? folderIdRef.current ?? null,
      );
      useTaskStore.getState().finishFamily('import');
    } catch (err) {
      useTaskStore.getState().failFamily('import');
      notifyError(err, 'Import Folder Failed');
    }
  }, [folderIdRef, folderImportDialog]);

  useEffect(() => {
    if (importRequestToken === importHandledToken) return;
    markImportHandled(importRequestToken);
    if (importRequestKind === 'folder') {
      void handleImportFolderRequest(importTargetFolderId);
      return;
    }
    void handleImport();
  }, [
    handleImport,
    handleImportFolderRequest,
    importHandledToken,
    importRequestKind,
    importRequestToken,
    importTargetFolderId,
    markImportHandled,
  ]);

  useEffect(() => {
    const webview = getCurrentWebview();
    const promise = webview.onDragDropEvent(async (event) => {
      const payload = event.payload as DragDropPayload;
      if (payload.type === 'enter') {
        setDragOver(!imageDrag.getPendingNativeDragHashes());
        return;
      }
      if (payload.type === 'leave') {
        setDragOver(false);
        return;
      }
      if (payload.type !== 'drop') return;

      setDragOver(false);
      const pendingHashes = imageDrag.getPendingNativeDragHashes();
      imageDrag.clearNativeDragSession();
      if (pendingHashes) return;

      // If a single path is dropped and it looks like a directory (no media extension),
      // show the folder import dialog so the user can choose to preserve structure.
      const paths = payload.paths;
      const mediaExtensions = /\.(jpe?g|png|gif|webp|bmp|tiff?|svg|mp4|mkv|webm|avi|mov|wmv|flv|m4v|psd|avif|jxl|ico|pdf)$/i;
      if (paths.length === 1 && !mediaExtensions.test(paths[0])) {
        setFolderImportDialog({
          path: paths[0],
          preserveStructure: true,
          targetFolderId: folderIdRef.current ?? null,
        });
        return;
      }

      try {
        await importPaths(paths);
      } catch (err) {
        useTaskStore.getState().failFamily('import');
        notifyError(err, 'Import Failed');
      }
    });
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [importPaths, setDragOver]);

  return {
    folderImportDialog,
    setFolderImportDialog,
    handleImport,
    handleImportFolderRequest,
    handleConfirmImportFolder,
  };
}
