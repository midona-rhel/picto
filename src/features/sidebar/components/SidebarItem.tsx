
import { type ReactNode, type MouseEvent, type DragEvent, useState, useCallback } from 'react';
import styles from './Sidebar.module.css';
import { imageDrag } from '../../shared/lib/imageDrag';

interface SidebarItemProps {
  icon: ReactNode;
  label?: string;
  count?: number | null;
  isActive?: boolean;
  isSelected?: boolean;
  isDropTarget?: boolean;
  isContextHighlight?: boolean;
  indent?: number;
  onClick?: (e: MouseEvent) => void;
  onContextMenu?: (e: MouseEvent) => void;
  onDoubleClick?: () => void;
  onNativeDrop?: (folderId: number, hashes: string[]) => void;
  /** Generic drop handler for internal hashes (e.g. status changes on Inbox/Trash) */
  onHashDrop?: (hashes: string[]) => void;
  style?: React.CSSProperties;
  className?: string;
  children?: ReactNode;
  /** data attribute for drag-drop targeting */
  dataFolderDropId?: number | null;
}

export function SidebarItem({
  icon,
  label,
  count,
  isActive,
  isSelected,
  isDropTarget,
  isContextHighlight,
  indent = 0,
  onClick,
  onContextMenu,
  onDoubleClick,
  onNativeDrop,
  onHashDrop,
  style,
  className,
  children,
  dataFolderDropId,
}: SidebarItemProps) {
  const [nativeDragOver, setNativeDragOver] = useState(false);
  const dropEnabled = dataFolderDropId != null || !!onHashDrop;

  const handleDragOver = useCallback((e: DragEvent) => {
    if (!dropEnabled || !imageDrag.getPendingNativeDragHashes()) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'move';
    setNativeDragOver(true);
  }, [dropEnabled]);

  const handleDragLeave = useCallback(() => {
    setNativeDragOver(false);
  }, []);

  const handleDrop = useCallback((e: DragEvent) => {
    setNativeDragOver(false);
    const hashes = imageDrag.getPendingNativeDragHashes();
    if (!hashes) return;
    e.preventDefault();
    e.stopPropagation();
    // PBI-053: Clear session immediately before invoking drop handler.
    imageDrag.clearNativeDragSession();
    if (onHashDrop) {
      onHashDrop(hashes);
    } else if (dataFolderDropId != null && onNativeDrop) {
      onNativeDrop(dataFolderDropId, hashes);
    }
  }, [dataFolderDropId, onNativeDrop, onHashDrop]);

  const cls = [
    styles.item,
    isActive && styles.itemActive,
    isSelected && styles.itemSelected,
    (isDropTarget || nativeDragOver) && styles.itemDropTarget,
    isContextHighlight && !isActive && styles.itemContextHighlight,
    className,
  ].filter(Boolean).join(' ');

  return (
    <div
      className={cls}
      onClick={onClick}
      onContextMenu={onContextMenu}
      onDoubleClick={onDoubleClick}
      onDragOver={dropEnabled ? handleDragOver : undefined}
      onDragLeave={dropEnabled ? handleDragLeave : undefined}
      onDrop={dropEnabled ? handleDrop : undefined}
      style={{ paddingLeft: indent * 20, ...style }}
      data-folder-drop-id={dataFolderDropId}
    >
      <span className={styles.itemIcon}>{icon}</span>
      {children ?? (
        <span className={styles.itemLabel}>{label}</span>
      )}
      {count != null && (
        <span className={styles.itemCount}>{count.toLocaleString()}</span>
      )}
    </div>
  );
}
