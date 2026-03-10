import { ConfirmModal } from '../../../shared/components/ConfirmModal';
import { ContextMenu } from '../../../shared/components/ContextMenu';
import { BatchRenameDialog } from '../../../shared/components/BatchRenameDialog';
import { GridDropOverlay } from './GridDropOverlay';
import type { MasonryImageItem } from '../shared';

export function GridDialogsLayer(props: {
  contextMenuState: {
    items: Parameters<typeof ContextMenu>[0]['items'];
    position: Parameters<typeof ContextMenu>[0]['position'];
  } | null;
  onCloseContextMenu: () => void;
  isDragOver: boolean;
  batchRenameOpen: boolean;
  onCloseBatchRename: () => void;
  batchRenameImages: MasonryImageItem[];
  folderImportDialog: { path: string; preserveStructure: boolean } | null;
  setFolderImportDialog: React.Dispatch<React.SetStateAction<{ path: string; preserveStructure: boolean } | null>>;
  onConfirmImportFolder: () => void | Promise<void>;
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
    </>
  );
}
