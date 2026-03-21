import { IconFolderPlus, IconFolderOpen } from '@tabler/icons-react';
import { StateActions, StateBlock } from '../../../shared/components/state';
import { TextButton } from '../../../shared/components/TextButton';
import { useLibraryStore } from '../../../state/libraryStore';
import { save as showSaveDialog } from '#desktop/api';

export function NoLibraryState() {
  const { createLibrary, openLibrary } = useLibraryStore();

  const handleCreate = async () => {
    const savePath = await showSaveDialog({
      title: 'Create New Library',
      defaultPath: 'My Library.library',
      properties: ['createDirectory'],
    });
    if (!savePath) return;
    const filename = savePath.split('/').pop() ?? 'Library';
    const name = filename.replace(/\.library$/, '');
    const dir = savePath.substring(0, savePath.lastIndexOf('/'));
    await createLibrary(name, dir);
  };

  const handleOpen = async () => {
    await openLibrary();
  };

  return (
    <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
      <StateBlock
        variant="empty"
        title="No library open"
        description="Create or open a library to get started."
        action={(
          <StateActions>
            <TextButton onClick={handleCreate}>
              <IconFolderPlus size={14} />
              Create Library
            </TextButton>
            <TextButton onClick={handleOpen}>
              <IconFolderOpen size={14} />
              Open Library
            </TextButton>
          </StateActions>
        )}
      />
    </div>
  );
}
