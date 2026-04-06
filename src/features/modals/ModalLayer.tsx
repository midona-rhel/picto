/**
 * ModalLayer — renders all application modals. Placed once in AppShell.
 */

import { useAtomValue, useSetAtom } from 'jotai';
import {
  confirmModalAtom,
  smartFolderModalAtom,
  folderWatchModalAtom,
  exportModalAtom,
  createGroupModalAtom,
  folderImportModalAtom,
} from '../../state/modals';
import { ConfirmModal } from './ConfirmModal';
import { SmartFolderModal } from './SmartFolderModal';
import { FolderWatchModal } from './FolderWatchModal';
import { ExportModal } from './ExportModal';
import { CreateGroupModal } from './CreateGroupModal';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { foldersController } from '../../controllers/foldersController';
import { subscriptionsController } from '../../controllers/subscriptionsController';
import * as api from '../../platform/api';

export function ModalLayer() {
  const confirm = useAtomValue(confirmModalAtom);
  const setConfirm = useSetAtom(confirmModalAtom);

  const smartFolder = useAtomValue(smartFolderModalAtom);
  const setSmartFolder = useSetAtom(smartFolderModalAtom);

  const folderWatch = useAtomValue(folderWatchModalAtom);
  const setFolderWatch = useSetAtom(folderWatchModalAtom);

  const exportState = useAtomValue(exportModalAtom);
  const setExport = useSetAtom(exportModalAtom);

  const createGroup = useAtomValue(createGroupModalAtom);
  const setCreateGroup = useSetAtom(createGroupModalAtom);

  const folderImport = useAtomValue(folderImportModalAtom);
  const setFolderImport = useSetAtom(folderImportModalAtom);

  return (
    <>
      <ConfirmModal
        open={confirm.open}
        onClose={() => setConfirm({ ...confirm, open: false })}
        onConfirm={() => { confirm.onConfirm(); setConfirm({ ...confirm, open: false }); }}
        title={confirm.title}
        message={confirm.message}
        confirmLabel={confirm.confirmLabel}
        danger={confirm.danger}
      />

      <SmartFolderModal
        open={smartFolder.open}
        onClose={() => setSmartFolder({ ...smartFolder, open: false })}
        onSave={(folder) => {
          if (smartFolder.mode === 'edit' && smartFolder.initial?.id) {
            void smartFoldersController.update(smartFolder.initial.id, folder);
          } else {
            void smartFoldersController.create(folder);
          }
          setSmartFolder({ ...smartFolder, open: false });
        }}
        initial={smartFolder.initial}
        mode={smartFolder.mode}
      />

      <FolderWatchModal
        open={folderWatch.open}
        onClose={() => setFolderWatch({ ...folderWatch, open: false })}
        onSave={(config) => {
          if (folderWatch.folderId != null) {
            void foldersController.setWatchConfig(folderWatch.folderId, {
              watchPath: config.watchPath,
              enabled: config.enabled,
              subfolders: config.subfolders,
              importStatusMode: config.importStatusMode,
            });
          }
          setFolderWatch({ ...folderWatch, open: false });
        }}
        onRemove={folderWatch.folderId != null ? () => {
          void foldersController.clearWatchConfig(folderWatch.folderId!);
          setFolderWatch({ ...folderWatch, open: false });
        } : undefined}
        initial={folderWatch.initial}
      />

      <ExportModal
        open={exportState.open}
        onClose={() => setExport({ open: false, fileCount: 0 })}
        onExport={(config) => {
          if (exportState.target) {
            void api.exportMedia(exportState.target, {
              output_dir: config.outputDir,
              format: config.format === 'original' ? null : config.format,
              quality: config.quality,
              width: config.width,
              height: config.height,
              keep_aspect: config.keepAspectRatio,
            });
          }
          setExport({ open: false, fileCount: 0 });
        }}
        fileCount={exportState.fileCount}
      />

      <CreateGroupModal
        open={createGroup.open}
        onClose={() => setCreateGroup({ open: false })}
        onCreate={(name, schedule) => {
          void subscriptionsController.createGroup(name, schedule);
          setCreateGroup({ open: false });
        }}
      />

      <ConfirmModal
        open={folderImport.open}
        onClose={() => setFolderImport({ ...folderImport, open: false })}
        onConfirm={() => {
          void api.importFolder(folderImport.path, {
            preserve_structure: folderImport.preserveStructure,
            parent_folder_id: folderImport.targetFolderId,
          });
          setFolderImport({ ...folderImport, open: false });
        }}
        title="Import Folder"
        confirmLabel="Import"
        message=""
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <p style={{ margin: 0 }}>
            Import <strong>{folderImport.path.split(/[\\/]/).filter(Boolean).pop() ?? 'folder'}</strong>
          </p>
          <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, color: 'var(--color-text-secondary)' }}>
            <input
              type="checkbox"
              checked={folderImport.preserveStructure}
              onChange={(e) => setFolderImport({ ...folderImport, preserveStructure: e.currentTarget.checked })}
            />
            Keep folder structure
          </label>
        </div>
      </ConfirmModal>
    </>
  );
}
