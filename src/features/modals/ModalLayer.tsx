/**
 * ModalLayer — renders all application modals. Placed once in AppShell.
 */

import { useAtomValue, useSetAtom } from 'jotai';
import { modalStyles } from '../../shared/ui/GlassModal';
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
import { TagSelectModal } from './TagSelectModal';
import { FolderPickerModal } from './FolderPickerModal';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { foldersController } from '../../controllers/foldersController';
import { subscriptionsController } from '../../controllers/subscriptionsController';
import { filesController } from '../../controllers/filesController';

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
            void filesController.exportMedia(exportState.target, {
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
          void filesController.addMedia([folderImport.path], {
            preserve_structure: true,
            parent_folder_id: folderImport.targetFolderId,
          });
          setFolderImport({ ...folderImport, open: false });
        }}
        title="Import Folder"
        confirmLabel="Import"
        message=""
      >
        <div className={modalStyles.stackSm}>
          <p className={modalStyles.helpText} style={{ fontSize: 13 }}>
            Import <strong>{folderImport.path.split(/[\\/]/).filter(Boolean).pop() ?? 'folder'}</strong>
          </p>
        </div>
      </ConfirmModal>

      <TagSelectModal />
      <FolderPickerModal />
    </>
  );
}
