import { ConfirmModal } from '../../../shared/components/ConfirmModal';
import { ContextMenu } from '../../../shared/components/ContextMenu';
import { BatchRenameDialog } from '../../../shared/components/BatchRenameDialog';
import type { ExportDialogState } from '../hooks/useGridExportActions';
import { ExportDialog } from './ExportDialog';
import { GridDropOverlay } from './GridDropOverlay';
import type { MasonryItem } from '../shared';

export function GridDialogsLayer(props: {
  contextMenuState: {
    items: Parameters<typeof ContextMenu>[0]['items'];
    position: Parameters<typeof ContextMenu>[0]['position'];
  } | null;
  onCloseContextMenu: () => void;
  isDragOver: boolean;
  batchRenameOpen: boolean;
  onCloseBatchRename: () => void;
  batchRenameImages: MasonryItem[];
  folderImportDialog: { path: string; preserveStructure: boolean; targetFolderId: number | null } | null;
  setFolderImportDialog: React.Dispatch<React.SetStateAction<{ path: string; preserveStructure: boolean; targetFolderId: number | null } | null>>;
  onConfirmImportFolder: () => void | Promise<void>;
  exportDialogOpen: boolean;
  exportDialogState: ExportDialogState;
  onCloseExportDialog: () => void;
  onExportDialogChange: (patch: Partial<ExportDialogState>) => void;
  onChooseExportDir: () => Promise<string | null>;
  onConfirmExport: () => void | Promise<void>;
  canConfirmExport: boolean;
}) {
  const {
    contextMenuState,
    onCloseContextMenu,
    isDragOver,
    batchRenameOpen,
    onCloseBatchRename,
    batchRenameImages,
    folderImportDialog,
    setFolderImportDialog,
    onConfirmImportFolder,
    exportDialogOpen,
    exportDialogState,
    onCloseExportDialog,
    onExportDialogChange,
    onChooseExportDir,
    onConfirmExport,
    canConfirmExport,
  } = props;

  return (
    <>
      {contextMenuState && (
        <ContextMenu
          items={contextMenuState.items}
          position={contextMenuState.position}
          onClose={onCloseContextMenu}
        />
      )}

      {isDragOver && <GridDropOverlay />}

      <BatchRenameDialog
        opened={batchRenameOpen}
        onClose={onCloseBatchRename}
        images={batchRenameImages}
      />

      <ConfirmModal
        opened={folderImportDialog != null}
        onClose={() => setFolderImportDialog(null)}
        onConfirm={() => { void onConfirmImportFolder(); }}
        title="Import Folder"
        confirmLabel="Import"
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <p style={{ margin: 0 }}>
            Import <strong>{folderImportDialog?.path.split(/[\\/]/).filter(Boolean).pop() ?? 'folder'}</strong>.
          </p>
          <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <input
              type="checkbox"
              checked={folderImportDialog?.preserveStructure ?? true}
              onChange={(event) => {
                const checked = event.currentTarget.checked;
                setFolderImportDialog((current) => (
                  current ? { ...current, preserveStructure: checked } : current
                ));
              }}
            />
            Keep the full folder structure and create it in Picto
          </label>
        </div>
      </ConfirmModal>

      <ExportDialog
        opened={exportDialogOpen}
        state={exportDialogState}
        onClose={onCloseExportDialog}
        onChange={onExportDialogChange}
        onChooseOutputDir={onChooseExportDir}
        onConfirm={onConfirmExport}
        canConfirm={canConfirmExport}
      />
    </>
  );
}
