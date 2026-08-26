import { useEffect, useState } from 'react';
import { foldersController } from '../../controllers/foldersController';
import { showErrorNotification } from '../../shared/lib/notifications';
import { GlassInput } from '../../shared/ui/GlassInput';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import { TagTokenInput } from '../../shared/ui/TagTokenInput';

export function FolderAutoTagsModal({
  open,
  folderIds,
  folderName,
  initialTags,
  onClose,
}: {
  open: boolean;
  folderIds: number[];
  folderName: string | null;
  initialTags: string[];
  onClose: () => void;
}) {
  const [tags, setTags] = useState<string[]>(initialTags);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open) return;
    setTags(initialTags);
    setSaving(false);
  }, [open, initialTags]);

  const save = () => {
    if (saving || folderIds.length === 0) return;
    setSaving(true);
    void Promise.all(folderIds.map((folderId) => foldersController.setAutoTags(folderId, tags)))
      .then(onClose)
      .catch((reason) => {
        setSaving(false);
        showErrorNotification({
          title: 'Could not set folder auto tags',
          message: reason instanceof Error ? reason.message : String(reason),
        });
      });
  };

  const displayName = folderName ?? `${folderIds.length} folders selected`;

  return (
    <GlassModal
      open={open}
      onClose={onClose}
      title="Set Auto Tags"
      size="sm"
      footer={
        <>
          <button className={modalStyles.btn} onClick={onClose} disabled={saving} type="button">
            Cancel
          </button>
          <button
            className={`${modalStyles.btn} ${modalStyles.btnPrimary}`}
            data-modal-primary="true"
            onClick={save}
            disabled={saving || folderIds.length === 0}
            type="button"
          >
            Save
          </button>
        </>
      }
    >
      <div className={modalStyles.stack}>
        <label className={modalStyles.field}>
          <span className={modalStyles.fieldLabel}>{folderIds.length === 1 ? 'Folder Name' : 'Folders'}</span>
          <GlassInput value={displayName} readOnly />
        </label>
        <label className={modalStyles.field}>
          <span className={modalStyles.fieldLabel}>Auto Tags</span>
          <TagTokenInput values={tags} onChange={setTags} autoFocus ariaLabel="Auto Tags" />
        </label>
      </div>
    </GlassModal>
  );
}
