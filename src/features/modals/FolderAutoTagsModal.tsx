import { useEffect, useState } from 'react';
import { useSetAtom } from 'jotai';
import { foldersController } from '../../controllers/foldersController';
import { showErrorNotification } from '../../shared/lib/notifications';
import { GlassModal } from '../../shared/ui/GlassModal';
import { TagAssignmentControl } from '../../shared/ui/TagAssignmentControl';
import { tagSelectPortalAtom } from '../../state/portals';

export function FolderAutoTagsModal({
  open,
  folderIds,
  initialTags,
  onClose,
}: {
  open: boolean;
  folderIds: number[];
  initialTags: string[];
  onClose: () => void;
}) {
  const [tags, setTags] = useState(initialTags);
  const openTagPicker = useSetAtom(tagSelectPortalAtom);

  useEffect(() => {
    if (open) setTags(initialTags);
  }, [open, initialTags]);

  const apply = (nextTags: string[]) => {
    setTags(nextTags);
    void Promise.all(folderIds.map((folderId) => foldersController.setAutoTags(folderId, nextTags)))
      .catch((reason) => {
        showErrorNotification({
          title: 'Could not set folder auto tags',
          message: reason instanceof Error ? reason.message : String(reason),
        });
      });
  };

  return (
    <GlassModal open={open} onClose={onClose} title="Set Auto Tags" size="sm">
      <TagAssignmentControl
        tags={tags}
        onRemove={(tag) => apply(tags.filter((current) => current !== tag))}
        onOpen={(button) => {
          const rect = button.getBoundingClientRect();
          openTagPicker({
            open: true,
            anchor: { x: rect.left, y: rect.top },
            anchorPlacement: 'above',
            selectedTags: tags,
            onApplyTags: apply,
          });
        }}
      />
    </GlassModal>
  );
}
