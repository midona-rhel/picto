/**
 * ModalLayer — renders all application modals. Placed once in AppShell.
 */

import { useAtomValue, useSetAtom } from 'jotai';
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
  updateModalAtom,
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
import { FolderImportModal } from './FolderImportModal';
import { UpdateModal } from './UpdateModal';

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
  const updateModal = useAtomValue(updateModalAtom);
  const setUpdateModal = useSetAtom(updateModalAtom);

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
        coverRootId={groupOrganizer.coverRootId}
        groups={groupOrganizer.groups}
        initialNotes={groupOrganizer.notes}
        maximumNoteBytes={groupOrganizer.notesMaximumBytes}
        onClose={() => setGroupOrganizer({
          open: false,
          target: null,
          coverRootId: null,
          groups: [],
          notes: '',
          notesMaximumBytes: 65_536,
        })}
        onBeforeSubmit={groupOrganizer.onBeforeSubmit}
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
        editor={smartFolder.editor}
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
        folderId={folderWatch.folderId ?? null}
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

      <FolderImportModal
        open={folderImport.open}
        onClose={() => setFolderImport({ ...folderImport, open: false })}
        path={folderImport.path}
        targetFolderId={folderImport.targetFolderId}
        onImport={(options) => {
          void filesController.addMedia([folderImport.path], {
            preserve_structure: options.preserveStructure,
            include_subfolders: options.includeSubfolders,
            include_folders_without_media: options.includeFoldersWithoutMedia,
            watch_source_folder: options.watchSourceFolder,
            parent_folder_id: folderImport.targetFolderId,
            lifecycle: folderImport.lifecycle,
          }).catch((reason) => {
            showErrorNotification({
              title: 'Could not import folder',
              message: reason instanceof Error ? reason.message : String(reason),
            });
          });
          setFolderImport({ ...folderImport, open: false });
        }}
      />

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
      <UpdateModal open={updateModal.open} onClose={() => setUpdateModal({ open: false })} />
    </>
  );
}
