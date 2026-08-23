import { useCallback, useEffect, useMemo, useState } from 'react';
import { IconPhoto, IconX } from '@tabler/icons-react';
import { useSetAtom } from 'jotai';
import { viewerController } from '../../controllers/viewerController';
import { MediaView } from '../viewer/MediaView';
import { CanvasGrid } from '../grid/canvas/CanvasGrid';
import { ContextMenu, useContextMenu, type MenuEntry } from '../../shared/ui/ContextMenu';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import type { ItemDetails } from '../../shared/types/generated/application/ItemDetails';
import type { MediaDetails } from '../../shared/types/generated/application/MediaDetails';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import { detachItems, reorderCollection, setCollectionCover } from '../../platform/entityApi';
import { viewerDisplayControlsAtom, viewerDisplayStateAtom } from '../../state/viewer';
import styles from './CollectionSurface.module.css';

export interface CollectionSurfaceProps {
  collectionId: number;
  initialMode?: 'reader' | 'editor';
  rootCurrentIndex: number;
  rootTotal: number;
  onNavigateRoot: (delta: number) => void;
  onClose: () => void;
}

function memberToGridItem(details: ItemDetails, media: MediaDetails): CanonicalEntityGridItem {
  return {
    item_id: media.media_item_id,
    kind: 'media',
    lifecycle: details.lifecycle,
    label: null,
    name: media.name,
    display_media_item_id: media.media_item_id,
    display_file_hash: media.file_hash,
    display_mime_type: media.mime_type,
    pixel_width: media.pixel_width,
    pixel_height: media.pixel_height,
    duration_ms: media.duration_ms,
    frame_count: media.frame_count,
    has_audio: media.has_audio,
    dominant_color_hex: media.dominant_color_hex,
    size_bytes: media.size_bytes,
    rating: media.rating,
    captured_at: media.captured_at,
    imported_at: media.imported_at,
    media_count: 1,
  };
}

function memberMenu(
  selected: CanonicalEntityGridItem[],
  onRemove: () => void,
  onCover: () => void,
): MenuEntry[] {
  const count = selected.length;
  return [
    {
      label: count > 1 ? `Remove ${count} from Collection` : 'Remove from Collection',
      icon: <IconX size={15} />,
      action: onRemove,
    },
    {
      label: 'Set as Cover',
      icon: <IconPhoto size={15} />,
      action: onCover,
      disabled: count !== 1,
    },
  ];
}

function CollectionImage({ item }: { item: CanonicalEntityGridItem }) {
  const [fullVisible, setFullVisible] = useState(false);
  const aspectRatio = item.pixel_width && item.pixel_height
    ? `${item.pixel_width} / ${item.pixel_height}`
    : undefined;

  return (
    <div className={styles.imageFrame} style={{ aspectRatio }}>
      <img
        className={styles.thumbnail}
        src={mediaThumbnailUrl(item.display_file_hash)}
        alt=""
        loading="lazy"
      />
      <img
        className={`${styles.fullImage} ${fullVisible ? styles.fullImageVisible : ''}`}
        src={mediaFileUrl(item.display_file_hash, item.display_mime_type)}
        alt={item.name ?? ''}
        loading="lazy"
        onLoad={(event) => {
          const image = event.currentTarget;
          const reveal = () => setFullVisible(true);
          if (typeof image.decode === 'function') image.decode().then(reveal).catch(reveal);
          else reveal();
        }}
      />
    </div>
  );
}

export function CollectionSurface({
  collectionId,
  initialMode = 'reader',
  rootCurrentIndex,
  rootTotal,
  onNavigateRoot,
  onClose,
}: CollectionSurfaceProps) {
  const [details, setDetails] = useState<ItemDetails | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState(initialMode);
  const [selectedItemIds, setSelectedItemIds] = useState<Set<number>>(new Set());
  const [viewerIndex, setViewerIndex] = useState<number | null>(null);
  const contextMenu = useContextMenu();
  const setDisplayState = useSetAtom(viewerDisplayStateAtom);
  const setDisplayControls = useSetAtom(viewerDisplayControlsAtom);

  const refresh = useCallback(async () => {
    try {
      const next = await viewerController.getItemDetails(collectionId);
      if (next.kind !== 'collection') {
        onClose();
        return;
      }
      setDetails(next);
      setSelectedItemIds((current) => {
        const memberIds = new Set(next.media.map((media) => media.media_item_id));
        return new Set([...current].filter((itemId) => memberIds.has(itemId)));
      });
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  }, [collectionId, onClose]);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(
    () => libraryInvalidation.register(`item:${collectionId}`, () => { void refresh(); }),
    [collectionId, refresh],
  );

  useEffect(() => {
    if (!details || viewerIndex !== null) return;
    setDisplayState({ currentIndex: rootCurrentIndex, total: rootTotal });
    setDisplayControls(mode === 'reader' ? {
      close: onClose,
      navigate: onNavigateRoot,
      edit: () => setMode('editor'),
    } : {
      close: () => {
        setMode('reader');
        setSelectedItemIds(new Set());
      },
      backLabel: 'Back to collection',
    });
    return () => {
      setDisplayState(null);
      setDisplayControls(null);
    };
  }, [details, mode, onClose, onNavigateRoot, rootCurrentIndex, rootTotal, setDisplayControls, setDisplayState, viewerIndex]);

  useEffect(() => {
    if (viewerIndex !== null) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
      if (mode === 'reader' && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
        event.preventDefault();
        onNavigateRoot(event.key === 'ArrowLeft' ? -1 : 1);
        return;
      }
      if (event.key !== 'Escape') return;
      event.preventDefault();
      if (mode === 'editor') {
        setMode('reader');
        setSelectedItemIds(new Set());
      } else {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [mode, onClose, onNavigateRoot, viewerIndex]);

  const members = useMemo(
    () => details?.media.map((media) => memberToGridItem(details, media)) ?? [],
    [details],
  );
  const selectedItems = useMemo(
    () => members.filter((item) => selectedItemIds.has(item.item_id)),
    [members, selectedItemIds],
  );

  const removeMembers = useCallback(async (selected: CanonicalEntityGridItem[]) => {
    if (selected.length === 0) return;
    try {
      await detachItems({
        collection_id: collectionId,
        media_item_ids: selected.map((item) => item.item_id),
      });
      setSelectedItemIds(new Set());
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [collectionId, refresh]);

  const chooseCover = useCallback(async (selected: CanonicalEntityGridItem[]) => {
    if (selected.length !== 1) return;
    try {
      await setCollectionCover({
        collection_id: collectionId,
        media_item_id: selected[0].item_id,
      });
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [collectionId, refresh]);

  const openMemberMenu = useCallback((
    item: CanonicalEntityGridItem,
    position: { x: number; y: number },
  ) => {
    const selected = selectedItemIds.has(item.item_id) ? selectedItems : [item];
    setSelectedItemIds(new Set(selected.map((selectedItem) => selectedItem.item_id)));
    contextMenu.openAt(position, memberMenu(
      selected,
      () => { void removeMembers(selected); },
      () => { void chooseCover(selected); },
    ));
  }, [chooseCover, contextMenu, removeMembers, selectedItemIds, selectedItems]);

  const saveOrder = useCallback(async (orderedItemIds: number[]) => {
    try {
      await reorderCollection({ collection_id: collectionId, media_item_ids: orderedItemIds });
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [collectionId, refresh]);

  if (loading && !details) {
    return <div className={styles.surface}><div className={styles.status}>Loading collection...</div></div>;
  }
  if (error && !details) {
    return <div className={styles.surface}><div className={`${styles.status} ${styles.error}`}>{error}</div></div>;
  }
  if (!details) return null;

  return (
    <section className={styles.surface} aria-label={details.label ?? 'Collection'}>
      {error && <div className={styles.inlineError}>{error}</div>}

      {mode === 'editor' ? (
        <div className={styles.editor}>
          <div className={styles.gridHost}>
            <CanvasGrid
              items={members}
              viewMode="grid"
              targetSize={180}
              showName
              showExtension={false}
              selectedItemIds={selectedItemIds}
              onSelectionChange={setSelectedItemIds}
              onTileClick={(_index, item, event) => {
                setSelectedItemIds((current) => {
                  if (event?.metaKey || event?.ctrlKey) {
                    const next = new Set(current);
                    if (next.has(item.item_id)) next.delete(item.item_id);
                    else next.add(item.item_id);
                    return next;
                  }
                  return new Set([item.item_id]);
                });
              }}
              onTileContextMenu={(_index, item, position) => openMemberMenu(item, position)}
              onReorder={(orderedItemIds) => { void saveOrder(orderedItemIds); }}
            />
          </div>
        </div>
      ) : (
        <main className={styles.reader}>
          {members.length === 0 ? (
            <div className={styles.empty}>This collection is empty.</div>
          ) : (
            <div className={styles.memberStack}>
              {members.map((item, index) => (
                <figure
                  className={styles.member}
                  data-collection-member={item.item_id}
                  key={item.item_id}
                  onDoubleClick={() => setViewerIndex(index)}
                >
                  {item.display_mime_type.startsWith('video/') ? (
                    <video
                      controls
                      preload="metadata"
                      poster={mediaThumbnailUrl(item.display_file_hash)}
                      src={mediaFileUrl(item.display_file_hash, item.display_mime_type)}
                    />
                  ) : (
                    <CollectionImage item={item} />
                  )}
                </figure>
              ))}
            </div>
          )}
        </main>
      )}

      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
        />
      )}
      {viewerIndex !== null && (
        <MediaView
          items={members}
          currentIndex={viewerIndex}
          totalCount={members.length}
          backLabel="Back to collection"
          recordItemId={collectionId}
          ratingItemId={null}
          onNavigate={(delta) => setViewerIndex((index) => Math.max(
            0,
            Math.min(members.length - 1, (index ?? 0) + delta),
          ))}
          onClose={() => setViewerIndex(null)}
        />
      )}
    </section>
  );
}
