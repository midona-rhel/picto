/**
 * ModalLayer — renders all application modals. Placed once in AppShell.
 */

import { useAtomValue, useSetAtom } from 'jotai';
import { modalStyles } from '../../shared/ui/GlassModal';
import {
  confirmModalAtom,
  smartFolderModalAtom,
  folderWatchModalAtom,
  folderAutoTagsModalAtom,
  exportModalAtom,
  batchRenameModalAtom,
  folderImportModalAtom,
  multiFileImportModalAtom,
  groupOrganizerModalAtom,
} from '../../state/modals';
import { ConfirmModal } from './ConfirmModal';
import { SmartFolderModal } from './SmartFolderModal';
import { FolderWatchModal } from './FolderWatchModal';
import { FolderAutoTagsModal } from './FolderAutoTagsModal';
import { ExportModal } from './ExportModal';
import { TagSelectModal } from './TagSelectModal';
import { FolderPickerModal } from './FolderPickerModal';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { foldersController } from '../../controllers/foldersController';
import { filesController } from '../../controllers/filesController';
import { GroupOrganizerModal } from './GroupOrganizerModal';
import { BatchRenameModal } from './BatchRenameModal';
import { setItemNames } from '../../controllers/entityMutations';
import { showErrorNotification } from '../../shared/lib/notifications';
import { LibraryCoverDialogHost } from '../library/LibraryCoverDialogHost';

export function ModalLayer() {
  const confirm = useAtomValue(confirmModalAtom);
  const setConfirm = useSetAtom(confirmModalAtom);

  const smartFolder = useAtomValue(smartFolderModalAtom);
  const setSmartFolder = useSetAtom(smartFolderModalAtom);

  const folderWatch = useAtomValue(folderWatchModalAtom);
  const setFolderWatch = useSetAtom(folderWatchModalAtom);
  const folderAutoTags = useAtomValue(folderAutoTagsModalAtom);
  const setFolderAutoTags = useSetAtom(folderAutoTagsModalAtom);

  const exportState = useAtomValue(exportModalAtom);
  const setExport = useSetAtom(exportModalAtom);
  const batchRename = useAtomValue(batchRenameModalAtom);
  const setBatchRename = useSetAtom(batchRenameModalAtom);

  const folderImport = useAtomValue(folderImportModalAtom);
  const setFolderImport = useSetAtom(folderImportModalAtom);
  const multiFileImport = useAtomValue(multiFileImportModalAtom);
  const setMultiFileImport = useSetAtom(multiFileImportModalAtom);
  const groupOrganizer = useAtomValue(groupOrganizerModalAtom);
  const setGroupOrganizer = useSetAtom(groupOrganizerModalAtom);

  const submitMultiFileImport = (groupFiles: boolean) => {
    const request = multiFileImport;
    setMultiFileImport({ ...request, open: false });
    void filesController.addMedia(request.paths, {
      lifecycle: request.lifecycle,
      parent_folder_id: request.parentFolderId,
      tags: request.tags,
      source_urls: request.sourceUrls,
      preserve_structure: request.preserveStructure,
      delete_after_ingest: request.deleteAfterIngest,
      group_files: groupFiles,
    }).catch((reason) => {
      showErrorNotification({
        title: 'Could not import media',
        message: reason instanceof Error ? reason.message : String(reason),
      });
    });
  };

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

      <GroupOrganizerModal
        open={groupOrganizer.open}
        target={groupOrganizer.target}
        groups={groupOrganizer.groups}
        onClose={() => setGroupOrganizer({ open: false, target: null, groups: [] })}
        onComplete={groupOrganizer.onComplete}
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

      <FolderAutoTagsModal
        open={folderAutoTags.open}
        folderIds={folderAutoTags.folderIds}
        initialTags={folderAutoTags.initialTags}
        onClose={() => setFolderAutoTags({ open: false, folderIds: [], initialTags: [] })}
      />

      <ExportModal
        open={exportState.open}
        onClose={() => setExport({ open: false, fileCount: 0 })}
        onExport={(config) => {
          if (exportState.target) {
            void filesController.exportMedia(exportState.target, {
              output_dir: config.outputDir,
              format: config.format,
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

      <BatchRenameModal
        open={batchRename.open}
        items={batchRename.items}
        onClose={() => setBatchRename({ open: false, items: [] })}
        onRename={(renames) => {
          void setItemNames(renames)
            .then(() => setBatchRename({ open: false, items: [] }))
            .catch((reason) => showErrorNotification({
              title: 'Could not rename items',
              message: reason instanceof Error ? reason.message : String(reason),
            }));
        }}
      />

      <LibraryCoverDialogHost />

      <ConfirmModal
        open={folderImport.open}
        onClose={() => setFolderImport({ ...folderImport, open: false })}
        onConfirm={() => {
          void filesController.addMedia([folderImport.path], {
            preserve_structure: true,
            parent_folder_id: folderImport.targetFolderId,
            lifecycle: folderImport.lifecycle,
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

      <ConfirmModal
        open={multiFileImport.open}
        onClose={() => setMultiFileImport({ ...multiFileImport, open: false })}
        onCancel={() => submitMultiFileImport(false)}
        onConfirm={() => submitMultiFileImport(true)}
        title="Import as Collection?"
        cancelLabel="No"
        confirmLabel="Yes"
        message={`Do you wish to import these ${multiFileImport.paths.length} media items as a collection?`}
      />

      <TagSelectModal />
      <FolderPickerModal />
    </>
  );
}
