
import { useCallback } from 'react';
import {
  IconPhoto,
  IconInbox,
  IconFolderQuestion,
  IconTrash,
  IconBookmarkQuestion,
  IconClock,
  IconBookmark,
  IconCopy,
  IconArrowsShuffle,
} from '@tabler/icons-react';

import { useDomainStore } from '../../../state/domainStore';
import { useNavigationStore } from '../../../state/navigationStore';
import { setStatusSelectionWithLifecycleEffects } from '../../../domain/actions/fileLifecycleActions';
import { SidebarJobStatus } from '../../layout/components/SidebarJobStatus';
import { runCriticalAction } from '../../../shared/lib/asyncOps';
import { FolderTree } from './FolderTree';
import { LibrarySwitcher } from './LibrarySwitcher';
import { SmartFolderList } from './SmartFolderList';
import { SidebarItem } from './SidebarItem';
import styles from './Sidebar.module.css';

interface SidebarProps {
  onSmartFolderUpdated?: () => void;
}

export function Sidebar({ onSmartFolderUpdated }: SidebarProps) {
  const { allImagesCount, inboxCount, uncategorizedCount, trashCount, untaggedCount, tagsCount, recentViewedCount, duplicatesCount } = useDomainStore();
  const { currentView, activeSmartFolder, activeFolder, activeStatusFilter, navigateTo } = useNavigationStore();

  const isAllImagesActive = !activeSmartFolder && !activeFolder && !activeStatusFilter && currentView === 'images';

  const handleStatusDrop = useCallback((hashes: string[], status: 'active' | 'inbox' | 'trash') => {
    runCriticalAction(
      'Move Failed',
      `sidebar status drop (${status})`,
      setStatusSelectionWithLifecycleEffects({ mode: 'explicit_hashes', hashes }, status),
    );
  }, []);

  const handleDropToAllImages = useCallback((hashes: string[]) => {
    handleStatusDrop(hashes, 'active');
  }, [handleStatusDrop]);

  const handleDropToInbox = useCallback((hashes: string[]) => {
    handleStatusDrop(hashes, 'inbox');
  }, [handleStatusDrop]);

  const handleDropToTrash = useCallback((hashes: string[]) => {
    handleStatusDrop(hashes, 'trash');
  }, [handleStatusDrop]);

  return (
    <div className={styles.sidebar}>
      <LibrarySwitcher />
      <div className={styles.scrollArea}>
        <SidebarItem
          icon={<IconPhoto size={16} />}
          label="All Images"
          count={allImagesCount}
          isActive={isAllImagesActive}
          onClick={() => navigateTo('images')}
          onHashDrop={handleDropToAllImages}
        />
        <SidebarItem
          icon={<IconInbox size={16} />}
          label="Inbox"
          count={inboxCount}
          isActive={currentView === 'images' && activeStatusFilter === 'inbox'}
          onClick={() => navigateTo('images', null, null, 'inbox')}
          onHashDrop={handleDropToInbox}
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
          icon={<IconClock size={16} />}
          label="Recently Viewed"
          count={recentViewedCount}
          isActive={currentView === 'images' && activeStatusFilter === 'recently_viewed'}
          onClick={() => navigateTo('images', null, null, 'recently_viewed')}
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
        />
        <FolderTree />

        <SmartFolderList onFolderUpdated={onSmartFolderUpdated} />
      </div>

      <SidebarJobStatus />
    </div>
  );
}
