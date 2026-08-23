/**
 * SubfolderGrid — collapsible section of folder tiles above the media grid.
 * Renders child folders as stacked-card tiles matching legacy v0.5.0-alpha exactly.
 *
 * Left-click selects, double-click navigates, right-click opens context menu.
 */

import { useState, useEffect, useCallback } from 'react';
import { IconChevronRight, IconFolder } from '@tabler/icons-react';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { foldersController } from '../../controllers/foldersController';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import styles from './SubfolderGrid.module.css';

interface SubfolderGridProps {
  childFolders: SidebarNodeDto[];
  targetSize: number;
  totalImageCount: number;
  onOpenFolder: (nodeId: string) => void;
  onSelectFolder?: (nodeId: string, event: React.MouseEvent) => void;
  onFolderContextMenu?: (nodeId: string, folder: SidebarNodeDto, position: { x: number; y: number }) => void;
  selectedNodeIds?: Set<string>;
}

export function SubfolderGrid({
  childFolders, targetSize, totalImageCount,
  onOpenFolder, onSelectFolder, onFolderContextMenu,
  selectedNodeIds,
}: SubfolderGridProps) {
  const [expanded, setExpanded] = useState(true);
  const [coverHashes, setCoverHashes] = useState<Map<number, string | null>>(new Map());

  // Fetch all child covers through one backend read instead of one IPC per tile.
  useEffect(() => {
    let cancelled = false;
    const folderIds = childFolders
      .map((folder) => parseFolderId(folder.id))
      .filter((folderId): folderId is number => folderId != null);
    if (folderIds.length === 0) {
      setCoverHashes(new Map());
      return;
    }
    void foldersController.getCoverHashes(folderIds).then((results) => {
      if (cancelled) return;
      const next = new Map<number, string | null>(folderIds.map((folderId) => [folderId, null]));
      for (const result of results) next.set(result.folder_id, result.entity_hash);
      setCoverHashes(next);
    }).catch(() => {
      if (!cancelled) setCoverHashes(new Map());
    });
    return () => { cancelled = true; };
  }, [childFolders]);

  const handleClick = useCallback((e: React.MouseEvent, nodeId: string) => {
    e.stopPropagation();
    onSelectFolder?.(nodeId, e);
  }, [onSelectFolder]);

  const handleDoubleClick = useCallback((e: React.MouseEvent, nodeId: string) => {
    e.stopPropagation();
    onOpenFolder(nodeId);
  }, [onOpenFolder]);

  const handleContextMenu = useCallback((e: React.MouseEvent, nodeId: string, folder: SidebarNodeDto) => {
    e.preventDefault();
    e.stopPropagation();
    onFolderContextMenu?.(nodeId, folder, { x: e.clientX, y: e.clientY });
  }, [onFolderContextMenu]);

  if (childFolders.length === 0) return null;

  const clampedSize = Math.max(150, Math.min(400, targetSize));
  const gridColumns = `repeat(auto-fill, minmax(${clampedSize}px, 1fr))`;

  return (
    <div className={styles.container}>
      {/* Section header */}
      <div className={styles.sectionLabel} onClick={() => setExpanded(!expanded)}>
        <span className={`${styles.chevron} ${!expanded ? styles.chevronCollapsed : ''}`}>
          <IconChevronRight size={11} />
        </span>
        Folders ({childFolders.length})
      </div>

      {expanded && (
        <div className={styles.list} style={{ gridTemplateColumns: gridColumns }}>
          {childFolders.map((folder) => {
            const folderId = parseFolderId(folder.id);
            const coverHash = folderId != null ? coverHashes.get(folderId) ?? null : null;
            const iconSize = Math.max(24, Math.round(clampedSize * 0.25));

            return (
              <div
                key={folder.id}
                className={`${styles.tile} ${selectedNodeIds?.has(folder.id) ? styles.tileSelected : ''}`}
                data-folder-hash={folder.id}
                onClick={(e) => handleClick(e, folder.id)}
                onDoubleClick={(e) => handleDoubleClick(e, folder.id)}
                onContextMenu={(e) => handleContextMenu(e, folder.id, folder)}
              >
                {/* Stacked card thumbnail */}
                <div className={styles.thumbnail}>
                  <div className={styles.pic1}>
                    {coverHash ? (
                      <img
                        className={styles.coverImage}
                        src={`media://localhost/thumb/${coverHash}.jpg`}
                        alt=""
                        loading="lazy"
                      />
                    ) : (
                      <div className={styles.folderPlaceholder}>
                        <IconFolder size={iconSize} color={folder.color ?? 'currentColor'} />
                      </div>
                    )}
                  </div>
                  <div className={styles.pic2} />
                  <div className={styles.pic3} />
                </div>

                {/* Name row */}
                <div className={styles.nameRow}>
                  {folder.icon ? (
                    <DynamicIcon name={folder.icon} size={19} color={folder.color ?? undefined} />
                  ) : (
                    <IconFolder size={19} color={folder.color ?? 'var(--color-text-tertiary)'} stroke={1.5} />
                  )}
                  <div className={styles.name}>{folder.name}</div>
                </div>

                {/* Metadata */}
                <div className={styles.metas}>
                  {folder.count ?? 0} {(folder.count ?? 0) === 1 ? 'item' : 'items'}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Content label */}
      {totalImageCount > 0 && (
        <div className={styles.contentLabel}>Content ({totalImageCount})</div>
      )}
    </div>
  );
}

function parseFolderId(nodeId: string): number | null {
  if (!nodeId.startsWith('folder:')) return null;
  const n = parseInt(nodeId.slice(7), 10);
  return isNaN(n) ? null : n;
}
