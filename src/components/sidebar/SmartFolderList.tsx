
import { useCallback, useState } from 'react';
import { useDisclosure } from '@mantine/hooks';
import { useInlineRename } from '../../shared/hooks/useInlineRename';
import { SmartFolderController } from '../../controllers/smartFolderController';
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
  DragOverlay,
  type DragStartEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
  arrayMove,
} from '@dnd-kit/sortable';

import { ContextMenu, useContextMenu } from '../../shared/components/ContextMenu';
import { SmartFolderModal } from '../smart-folders/SmartFolderModal';
import { DynamicIcon, DEFAULT_FOLDER_ICON } from '../smart-folders/iconRegistry';
import type { SmartFolder } from '../smart-folders/types';
import { folderToRust } from '../smart-folders/types';
import { FolderController } from '../../controllers/folderController';
import { registerUndoAction } from '../../controllers/undoRedoController';
import { SidebarController } from '../../controllers/sidebarController';
import { useDomainStore } from '../../state/domainStore';
import { useNavigationStore } from '../../state/navigationStore';
import { SidebarSection } from './SidebarSection';
import { SidebarItem } from './SidebarItem';
import { buildSmartFolderItemMenu } from '../../shared/components/context-actions/smartFolderActions';
import styles from './Sidebar.module.css';

interface SmartFolderListProps {
  onFolderUpdated?: () => void;
}

type DropPosition = 'before' | 'after';
interface DropIndicator { folderId: string; position: DropPosition; }

/** Sortable wrapper for a smart folder row */
function SortableSmartFolderRow({
  folder,
  children,
  dropIndicator,
}: {
  folder: SmartFolder;
  children: React.ReactNode;
  dropIndicator: DropIndicator | null;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    isDragging,
  } = useSortable({ id: `smart_folder:${folder.id}` });

  const style: React.CSSProperties = {
    opacity: isDragging ? 0.4 : 1,
    position: 'relative' as const,
  };

  const isDropTarget = dropIndicator != null && dropIndicator.folderId === folder.id;
  const isDropBefore = isDropTarget && dropIndicator.position === 'before';
  const isDropAfter = isDropTarget && dropIndicator.position === 'after';

  return (
    <div ref={setNodeRef} style={style} {...attributes} {...listeners}>
      {isDropBefore && <div className={styles.dropLine} style={{ top: 0 }} />}
      {children}
      {isDropAfter && <div className={styles.dropLine} style={{ bottom: 0 }} />}
    </div>
  );
}

export function SmartFolderList({ onFolderUpdated }: SmartFolderListProps) {
  const { smartFolders: domainFolders, smartFolderCounts: counts } = useDomainStore();
  const { activeSmartFolder, navigateToSmartFolder, navigateTo } = useNavigationStore();

  const folders: SmartFolder[] = domainFolders.map((sf) => ({
    id: sf.id,
    name: sf.name,
    icon: sf.icon ?? undefined,
    color: sf.color ?? undefined,
    predicate: sf.predicate as SmartFolder['predicate'],
    sort_field: sf.sort_field ?? undefined,
    sort_order: sf.sort_order ?? undefined,
  }));

  const [modalOpen, { open: openModal, close: closeModal }] = useDisclosure(false);
  const [editingFolder, setEditingFolder] = useState<SmartFolder | null>(null);
  const contextMenu = useContextMenu();

  const updateFolder = useCallback(async (
    folder: SmartFolder,
    updates: Partial<SmartFolder>,
    options?: { recordUndo?: boolean },
  ) => {
    if (!folder.id) return;
    try {
      const updated = { ...folder, ...updates };
      await SmartFolderController.update(folder.id, folderToRust(updated));
      if (options?.recordUndo !== false) {
        const before = { ...folder };
        registerUndoAction({
          label: 'Update smart folder',
          undo: async () => {
            await updateFolder(updated, before, { recordUndo: false });
          },
          redo: async () => {
            await updateFolder(before, updates, { recordUndo: false });
          },
        });
      }
      SidebarController.fetchInitialTree();
      onFolderUpdated?.();
    } catch (e) { console.error('Update failed:', e); }
  }, [onFolderUpdated]);

  const handleRenameCommit = useCallback(async (id: string, newName: string) => {
    const folder = folders.find((f) => f.id === id);
    if (folder) await updateFolder(folder, { name: newName });
  }, [folders, updateFolder]);
  const {
    renamingId: renamingFolderId, renameValue, startRename, setRenameValue,
    commitRename, renameInputRef, renameKeyHandler,
  } = useInlineRename(handleRenameCommit);

  const [contextMenuFolderId, setContextMenuFolderId] = useState<string | null>(null);

  const [activeId, setActiveId] = useState<string | null>(null);
  const [dropIndicator, setDropIndicator] = useState<DropIndicator | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );

  const handleContextMenu = (e: React.MouseEvent, folder: SmartFolder) => {
    const currentSortField = folder.sort_field ?? 'imported_at';
    const currentSortOrder: 'asc' | 'desc' = folder.sort_order === 'asc' ? 'asc' : 'desc';
    const items = buildSmartFolderItemMenu({
      editSmartFolder: () => {
        setEditingFolder(folder);
        openModal();
      },
      renameSmartFolder: () => {
        if (folder.id) startRename(folder.id, folder.name);
      },
      setSortField: (field) => {
        void updateFolder(folder, { sort_field: field });
      },
      setSortOrder: (order) => {
        void updateFolder(folder, { sort_order: order });
      },
      currentSortField,
      currentSortOrder,
      duplicateSmartFolder: async () => {
        try {
          let created = await SmartFolderController.create(folderToRust({ ...folder, id: undefined, name: `${folder.name} (copy)` }));
          registerUndoAction({
            label: 'Duplicate smart folder',
            undo: async () => {
              if (created?.id) await SmartFolderController.delete(created.id);
              SidebarController.fetchInitialTree();
              onFolderUpdated?.();
            },
            redo: async () => {
              created = await SmartFolderController.create(folderToRust({ ...folder, id: undefined, name: `${folder.name} (copy)` }));
              SidebarController.fetchInitialTree();
              onFolderUpdated?.();
            },
          });
          SidebarController.fetchInitialTree();
          onFolderUpdated?.();
        } catch (error) {
          console.error('Duplicate failed:', error);
        }
      },
      iconValue: folder.icon ?? null,
      colorValue: folder.color ?? null,
      onIconChange: (icon) => {
        void updateFolder(folder, { icon });
      },
      onColorChange: (color) => {
        void updateFolder(folder, { color });
      },
      deleteSmartFolder: async () => {
        if (!folder.id) return;
        try {
          const snapshot = { ...folder };
          await SmartFolderController.delete(folder.id);
          let recreated: SmartFolder | null = null;
          registerUndoAction({
            label: 'Delete smart folder',
            undo: async () => {
              recreated = await SmartFolderController.create(folderToRust({ ...snapshot, id: undefined }));
              SidebarController.fetchInitialTree();
              onFolderUpdated?.();
            },
            redo: async () => {
              const id = recreated?.id ?? snapshot.id;
              if (id) await SmartFolderController.delete(id);
              SidebarController.fetchInitialTree();
              onFolderUpdated?.();
            },
          });
          SidebarController.fetchInitialTree();
          onFolderUpdated?.();
          if (activeSmartFolder?.id === folder.id) navigateTo('images');
        } catch (error) {
          console.error('Delete failed:', error);
        }
      },
    });
    contextMenu.open(e, items);
  };

  const sortableIds = folders.map((f) => `smart_folder:${f.id}`);

  const handleDragStart = useCallback((event: DragStartEvent) => {
    setActiveId(String(event.active.id));
  }, []);

  const handleDragMove = useCallback((event: DragMoveEvent) => {
    const { active, over } = event;
    if (!over || active.id === over.id) {
      setDropIndicator(null);
      return;
    }
    const overFolder = folders.find((f) => `smart_folder:${f.id}` === over.id);
    if (!overFolder || !overFolder.id) { setDropIndicator(null); return; }

    const overRect = over.rect;
    const cursorY = event.activatorEvent instanceof MouseEvent
      ? event.activatorEvent.clientY + (event.delta?.y ?? 0)
      : overRect.top + overRect.height / 2;
    const ratio = (cursorY - overRect.top) / overRect.height;
    setDropIndicator({ folderId: overFolder.id, position: ratio < 0.5 ? 'before' : 'after' });
  }, [folders]);

  const handleDragEnd = useCallback((event: DragEndEvent) => {
    const savedIndicator = dropIndicator;
    setActiveId(null);
    setDropIndicator(null);
    const { active, over } = event;
    if (!over || active.id === over.id) return;

    const oldIndex = folders.findIndex((f) => `smart_folder:${f.id}` === active.id);
    let newIndex = folders.findIndex((f) => `smart_folder:${f.id}` === over.id);
    if (oldIndex === -1 || newIndex === -1) return;

    if (savedIndicator?.position === 'after') {
      if (oldIndex > newIndex) {
        newIndex = newIndex + 1;
      }
    } else if (savedIndicator?.position === 'before') {
      if (oldIndex < newIndex) {
        newIndex = newIndex - 1;
      }
    }

    if (oldIndex === newIndex) return;

    const previousMoves: [number, number][] = folders.map((f, i) => [parseInt(f.id!, 10), (i + 1) * 1000]);
    const reordered = arrayMove(folders, oldIndex, newIndex);
    const moves: [number, number][] = reordered.map((f, i) => [parseInt(f.id!, 10), (i + 1) * 1000]);
    FolderController.reorderSmartFolders(moves).then(() => {
      registerUndoAction({
        label: 'Reorder smart folders',
        undo: async () => {
          await FolderController.reorderSmartFolders(previousMoves);
          onFolderUpdated?.();
        },
        redo: async () => {
          await FolderController.reorderSmartFolders(moves);
          onFolderUpdated?.();
        },
      });
      onFolderUpdated?.();
    }).catch(console.error);
  }, [folders, dropIndicator, onFolderUpdated]);

  const handleDragCancel = useCallback(() => {
    setActiveId(null);
    setDropIndicator(null);
  }, []);

  const activeFolder = activeId ? folders.find((f) => `smart_folder:${f.id}` === activeId) : null;

  return (
    <>
      <SidebarSection title="Smart Folders" onAdd={() => { setEditingFolder(null); openModal(); }}>
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragStart={handleDragStart}
          onDragMove={handleDragMove}
          onDragEnd={handleDragEnd}
          onDragCancel={handleDragCancel}
        >
          <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
            {folders.map((folder) => {
              const isActive = activeSmartFolder?.id === folder.id;
              const count = folder.id ? counts[folder.id] : undefined;
              const isRenaming = renamingFolderId === folder.id;
              const iconName = folder.icon ?? DEFAULT_FOLDER_ICON;
              const folderColor = folder.color ?? 'currentColor';

              const row = (
                <SidebarItem
                  icon={<DynamicIcon name={iconName} size={18} color={folderColor} />}
                  label={isRenaming ? undefined : folder.name}
                  count={isRenaming ? null : count}
                  isActive={isActive}
                  isContextHighlight={contextMenuFolderId === folder.id && !isActive}
                  onClick={() => { if (!isRenaming && !isActive) navigateToSmartFolder(folder); }}
                  onContextMenu={(e) => { setContextMenuFolderId(folder.id ?? null); handleContextMenu(e, folder); }}
                >
                  {isRenaming ? (
                    <input
                      ref={renameInputRef}
                      className={styles.renameInput}
                      value={renameValue}
                      onChange={(e) => setRenameValue(e.target.value)}
                      onBlur={commitRename}
                      onKeyDown={renameKeyHandler}
                      onClick={(e) => e.stopPropagation()}
                    />
                  ) : undefined}
                </SidebarItem>
              );

              return (
                <SortableSmartFolderRow key={folder.id} folder={folder} dropIndicator={dropIndicator}>
                  {row}
                </SortableSmartFolderRow>
              );
            })}
          </SortableContext>

          <DragOverlay dropAnimation={null}>
            {activeFolder ? (
              <div className={styles.dragOverlay}>
                <SidebarItem
                  icon={<DynamicIcon name={activeFolder.icon ?? DEFAULT_FOLDER_ICON} size={18} color={activeFolder.color ?? 'currentColor'} />}
                  label={activeFolder.name}
                />
              </div>
            ) : null}
          </DragOverlay>
        </DndContext>
      </SidebarSection>

      {contextMenu.state && (
        <ContextMenu
          items={contextMenu.state.items}
          position={contextMenu.state.position}
          onClose={() => { contextMenu.close(); setContextMenuFolderId(null); }}
        />
      )}

      <SmartFolderModal
        opened={modalOpen}
        onClose={closeModal}
        folder={editingFolder}
        onSaved={() => onFolderUpdated?.()}
      />
    </>
  );
}
