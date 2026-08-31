import { forwardRef, useState, useEffect, useCallback, useImperativeHandle, useRef } from 'react';
import { IconChevronRight, IconFolder } from '@tabler/icons-react';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { foldersController } from '../../controllers/foldersController';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import styles from './SubfolderGrid.module.css';
import { ThumbnailImage } from '../../shared/ui/ThumbnailImage/ThumbnailImage';
import { GlassInput } from '../../shared/ui/GlassInput/GlassInput';
import { useAtomValue } from 'jotai';
import { gridSpacingAtom } from '../../state/grid';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import { t } from '../../i18n';

interface SubfolderGridProps {
  childFolders: SidebarNodeDto[];
  targetSize: number;
  totalImageCount: number;
  onOpenFolder: (nodeId: string) => void;
  onSelectFolder?: (nodeId: string, event: React.MouseEvent) => void;
  onFolderContextMenu?: (nodeId: string, folder: SidebarNodeDto, position: { x: number; y: number }) => void;
  selectedNodeIds?: Set<string>;
  renamingNodeId?: string | null;
  onRenameFolder?: (nodeId: string, name: string) => void;
  onCancelRename?: () => void;
}

export interface SubfolderGridHandle {
  collectMarqueeHits(rect: { left: number; top: number; width: number; height: number }): Set<string>;
}

export const SubfolderGrid = forwardRef<SubfolderGridHandle, SubfolderGridProps>(function SubfolderGrid({
  childFolders, targetSize, totalImageCount,
  onOpenFolder, onSelectFolder, onFolderContextMenu,
  selectedNodeIds, renamingNodeId, onRenameFolder, onCancelRename,
}, ref) {
  const spacing = useAtomValue(gridSpacingAtom);
  const [expanded, setExpanded] = useState(true);
  const rootRef = useRef<HTMLDivElement>(null);
  const [covers, setCovers] = useState<Map<number, { hash: string; mime: string }>>(new Map());
  const [coverRevision, setCoverRevision] = useState(0);
  const coverRequestKey = childFolders
    .map((folder) => parseFolderId(folder.id))
    .filter((folderId): folderId is number => folderId != null)
    .join(',');

  useEffect(
    () => libraryInvalidation.register('folders', () => setCoverRevision((revision) => revision + 1)),
    [],
  );

  useEffect(() => {
    let cancelled = false;
    const folderIds = coverRequestKey === ''
      ? []
      : coverRequestKey.split(',').map(Number);
    if (folderIds.length === 0) {
      setCovers(new Map());
      return;
    }
    const requestedFolderIds = new Set(folderIds);
    setCovers((current) => new Map(
      [...current].filter(([folderId]) => requestedFolderIds.has(folderId)),
    ));
    void foldersController.getCoverHashes(folderIds).then((results) => {
      if (cancelled) return;
      const next = new Map<number, { hash: string; mime: string }>();
      for (const result of results) {
        if (result.entity_hash && result.mime_type) {
          next.set(result.folder_id, { hash: result.entity_hash, mime: result.mime_type });
        }
      }
      setCovers(next);
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [coverRequestKey, coverRevision]);

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

  useImperativeHandle(ref, () => ({
    collectMarqueeHits(rect) {
      const root = rootRef.current;
      const hits = new Set<string>();
      if (!root) return hits;
      for (const tile of root.querySelectorAll<HTMLElement>('[data-folder-hash]')) {
        const id = tile.dataset.folderHash;
        const top = tile.offsetTop - root.offsetHeight;
        if (id && tile.offsetLeft + tile.offsetWidth > rect.left && tile.offsetLeft < rect.left + rect.width
          && top + tile.offsetHeight > rect.top && top < rect.top + rect.height) hits.add(id);
      }
      return hits;
    },
  }), []);

  if (childFolders.length === 0) return null;

  const clampedSize = Math.max(150, Math.min(400, targetSize));
  const gridColumns = `repeat(auto-fill, minmax(${clampedSize}px, 1fr))`;

  return (
    <div ref={rootRef} className={`${styles.container} ${spacing === 'tight' ? styles.tight : ''}`}>
      <div className={styles.sectionLabel} data-grid-header-interactive onClick={() => setExpanded(!expanded)}>
        <span className={`${styles.chevron} ${!expanded ? styles.chevronCollapsed : ''}`}>
          <IconChevronRight size={11} />
        </span>
        {t("Folders (")}{childFolders.length})
      </div>

      {expanded && (
        <div className={styles.list} style={{ gridTemplateColumns: gridColumns }}>
          {childFolders.map((folder) => {
            const folderId = parseFolderId(folder.id);
            const cover = folderId != null ? covers.get(folderId) ?? null : null;
            const iconSize = Math.max(24, Math.round(clampedSize * 0.25));

            return (
              <div
                key={folder.id}
                className={`${styles.tile} ${selectedNodeIds?.has(folder.id) ? styles.tileSelected : ''}`}
                data-folder-hash={folder.id}
                data-grid-header-interactive
                onClick={(e) => handleClick(e, folder.id)}
                onDoubleClick={(e) => handleDoubleClick(e, folder.id)}
                onContextMenu={(e) => handleContextMenu(e, folder.id, folder)}
              >
                <div className={styles.thumbnail}>
                  <div className={styles.pic1}>
                    {cover ? (
                      <ThumbnailImage
                        className={styles.coverImage}
                        src={`media://localhost/thumb/${cover.hash}.jpg`}
                        alt=""
                        loading="lazy"
                        fallback={cover.mime.startsWith('font/') ? 'font' : 'broken'}
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

                <div className={styles.nameRow}>
                  {folder.icon ? (
                    <DynamicIcon name={folder.icon} size={19} color={folder.color ?? undefined} />
                  ) : (
                    <IconFolder size={19} color={folder.color ?? 'var(--color-text-tertiary)'} stroke={1.5} />
                  )}
                  {renamingNodeId === folder.id ? (
                    <FolderRenameInput
                      key={folder.id}
                      folder={folder}
                      onCommit={(name) => onRenameFolder?.(folder.id, name)}
                      onCancel={onCancelRename}
                    />
                  ) : (
                    <div className={styles.name}>{folder.name}</div>
                  )}
                </div>

                <div className={styles.metas}>
                  {folder.count ?? 0} {(folder.count ?? 0) === 1 ? t("item") : t("items")}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {totalImageCount > 0 && (
        <div className={styles.contentLabel}>{t("Content (")}{totalImageCount})</div>
      )}
    </div>
  );
});

function FolderRenameInput({
  folder,
  onCommit,
  onCancel,
}: {
  folder: SidebarNodeDto;
  onCommit: (name: string) => void;
  onCancel?: () => void;
}) {
  const [value, setValue] = useState(folder.name);
  const commit = () => {
    const name = value.trim();
    if (name) onCommit(name);
    else onCancel?.();
  };
  return (
    <GlassInput
      autoFocus
      aria-label={t("Rename {value0}", { value0: folder.name })}
      value={value}
      style={{
        width: 'min(180px, 100%)',
        minWidth: 0,
        height: 24,
        padding: '2px 6px',
        textAlign: 'center',
      }}
      onFocus={(event) => event.currentTarget.select()}
      onChange={(event) => setValue(event.target.value)}
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === 'Enter') {
          event.preventDefault();
          commit();
        } else if (event.key === 'Escape') {
          event.preventDefault();
          onCancel?.();
        }
      }}
    />
  );
}

function parseFolderId(nodeId: string): number | null {
  if (!nodeId.startsWith('folder:')) return null;
  const n = parseInt(nodeId.slice(7), 10);
  return isNaN(n) ? null : n;
}
