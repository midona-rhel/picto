
import { useCallback, useEffect } from 'react';
import {
  IconPhoto,
  IconInbox,
  IconFolderQuestion,
  IconTrash,
  IconBookmarkQuestion,
  IconBookmark,
  IconCopy,
  IconArrowsShuffle,
} from '@tabler/icons-react';

import { useAtomValue } from 'jotai';
import { scopeCountsAtom, tagsCountAtom } from '../../../state/sidebar';
import { useNavigationStore } from '../../../state-legacy/navigationStore';
import { useLibraryStore } from '../../../state-legacy/libraryStore';
import { entityController } from '../../../controllers/entityController';
import { SidebarJobStatus } from '../../layout/components/SidebarJobStatus';
import { imageDrag } from '../../../shared/lib/imageDrag';
import { FolderTree } from './FolderTree';
import { LibrarySwitcher } from './LibrarySwitcher';
import { SmartFolderList } from './SmartFolderList';
import { SidebarItem } from './SidebarItem';
import styles from './Sidebar.module.css';

interface SidebarProps {
  onSmartFolderUpdated?: () => void;
}

export function Sidebar({ onSmartFolderUpdated }: SidebarProps) {
  const libraryPath = useLibraryStore((s) => s.currentPath);
  const noLibrary = !libraryPath;
  const { active: allActiveCount, inbox: inboxCount, trash: trashCount, uncategorized: uncategorizedCount, untagged: untaggedCount, duplicates: duplicatesCount } = useAtomValue(scopeCountsAtom);
  const tagsCount = useAtomValue(tagsCountAtom);
  const { currentView, activeSmartFolderId, activeFolderId, activeStatusFilter, navigateTo } = useNavigationStore();

  const isAllActiveScope = !activeSmartFolderId && activeFolderId == null && !activeStatusFilter && currentView === 'images';

  const handleStatusDrop = useCallback((hashes: string[], status: 'active' | 'inbox' | 'trash') => {
    // Infer source status from current view for undo
    const prevStatus: string = activeStatusFilter === 'inbox' ? 'inbox'
      : activeStatusFilter === 'trash' ? 'trash'
      : 'active';
    const spec = {
      mode: 'explicit_hashes' as const,
      hashes,
      scope: { kind: 'system' as const, system_key: 'all' as const },
      filters: {},
      sort: {},
      excluded_hashes: null,
      included_hashes: null,
    };
    entityController.changeStatusSelection(spec, status, prevStatus)
      .catch((err) => console.error('Status drop failed:', err));
  }, [activeStatusFilter]);

  const handleDropToAllActive = useCallback((hashes: string[]) => {
    handleStatusDrop(hashes, 'active');
  }, [handleStatusDrop]);

  const handleDropToInbox = useCallback((hashes: string[]) => {
    handleStatusDrop(hashes, 'inbox');
  }, [handleStatusDrop]);

  const handleDropToTrash = useCallback((hashes: string[]) => {
    handleStatusDrop(hashes, 'trash');
  }, [handleStatusDrop]);

  // Register internal drag status drop handler (pointer-based drag from grid)
  useEffect(() => {
    return imageDrag.onStatusDrop(({ hashes, status }) => {
      if (status === 'active' || status === 'inbox' || status === 'trash') {
        handleStatusDrop(hashes, status);
      }
    });
  }, [handleStatusDrop]);

  return (
    <div className={styles.sidebar}>
      <LibrarySwitcher />
      <div className={styles.scrollArea} style={noLibrary ? { pointerEvents: 'none', opacity: 0.4 } : undefined}>
        <SidebarItem
          icon={<IconPhoto size={16} />}
          label="All Active"
          count={allActiveCount}
          isActive={isAllActiveScope}
          onClick={() => navigateTo('images')}
          onHashDrop={handleDropToAllActive}
          dataStatusDrop="active"
        />
        <SidebarItem
          icon={<IconInbox size={16} />}
          label="Inbox"
          count={inboxCount}
          isActive={currentView === 'images' && activeStatusFilter === 'inbox'}
          onClick={() => navigateTo('images', null, null, 'inbox')}
          onHashDrop={handleDropToInbox}
          dataStatusDrop="inbox"
        />
        <SidebarItem
          icon={<IconFolderQuestion size={16} />}
          label="Uncategorized"
          count={uncategorizedCount}
          isActive={currentView === 'images' && activeStatusFilter === 'uncategorized'}
          onClick={() => navigateTo('images', null, null, 'uncategorized')}
        />
        <SidebarItem
          icon={<IconBookmarkQuestion size={16} />}
          label="Untagged"
          count={untaggedCount}
          isActive={currentView === 'images' && activeStatusFilter === 'untagged'}
          onClick={() => navigateTo('images', null, null, 'untagged')}
        />
        <SidebarItem
          icon={<IconBookmark size={16} />}
          label="Tag Manager"
          count={tagsCount}
          isActive={currentView === 'tags'}
          onClick={() => navigateTo('tags')}
        />
        <SidebarItem
          icon={<IconArrowsShuffle size={16} />}
          label="Random"
          isActive={currentView === 'images' && activeStatusFilter === 'random'}
          onClick={() => navigateTo('images', null, null, 'random')}
        />
        <SidebarItem
          icon={<IconCopy size={16} />}
          label="Duplicates"
          count={duplicatesCount}
          isActive={currentView === 'duplicates'}
          onClick={() => navigateTo('duplicates')}
        />
        <SidebarItem
          icon={<IconTrash size={16} />}
          label="Trash"
          count={trashCount}
          isActive={currentView === 'images' && activeStatusFilter === 'trash'}
          onClick={() => navigateTo('images', null, null, 'trash')}
          onHashDrop={handleDropToTrash}
          dataStatusDrop="trash"
        />
        <FolderTree />

        <SmartFolderList onFolderUpdated={onSmartFolderUpdated} />
      </div>

      <SidebarJobStatus />
    </div>
  );
}
