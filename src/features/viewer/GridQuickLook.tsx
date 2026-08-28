import { useCallback, useEffect, useState } from 'react';
import { mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import type { CanonicalEntityGridItem } from '../../shared/types/canonical';
import { GroupQuickLookContent } from '../groups/GroupQuickLook';
import { detailRendererKind } from './document/detailRendererKind';
import { QuickLookContent } from './QuickLook';
import { QuickLookHost } from './QuickLookHost';

interface GridQuickLookProps {
  items: CanonicalEntityGridItem[];
  currentIndex: number;
  totalCount?: number | null;
  onNavigate: (delta: number) => void;
  onClose: (exitItemId: number) => void;
  onLoadMore?: () => void;
}

export function GridQuickLook(props: GridQuickLookProps) {
  const [displayedIndex, setDisplayedIndex] = useState(props.currentIndex);
  const [decodedThumbnailItemId, setDecodedThumbnailItemId] = useState<number | null>(null);
  const currentItem = props.items[displayedIndex] ?? null;
  const [mediaReady, setMediaReady] = useState(false);
  const markMediaReady = useCallback(() => setMediaReady(true), []);

  useEffect(() => {
    if (props.currentIndex === displayedIndex) return;
    const displayedItem = props.items[displayedIndex] ?? null;
    const requestedItem = props.items[props.currentIndex] ?? null;
    if (!requestedItem) return;

    const isGroupToImage = displayedItem?.kind === 'collection'
      && requestedItem.kind !== 'collection'
      && detailRendererKind(requestedItem.mime) === 'image';
    if (!isGroupToImage) {
      setDecodedThumbnailItemId(null);
      setDisplayedIndex(props.currentIndex);
      return;
    }

    let cancelled = false;
    const image = new Image();
    const commit = () => {
      if (cancelled) return;
      setDecodedThumbnailItemId(requestedItem.root_id);
      setDisplayedIndex(props.currentIndex);
    };
    image.onload = () => {
      if (typeof image.decode === 'function') image.decode().then(commit).catch(commit);
      else commit();
    };
    image.onerror = commit;
    image.src = mediaThumbnailUrl(requestedItem.content_hash);
    return () => { cancelled = true; };
  }, [displayedIndex, props.currentIndex, props.items]);

  if (!currentItem) return null;

  const totalCount = props.totalCount ?? props.items.length;
  return (
    <QuickLookHost
      contentReady={currentItem.kind === 'collection' || mediaReady}
      currentIndex={displayedIndex}
      totalCount={totalCount}
      canPrevious={displayedIndex > 0}
      canNext={displayedIndex < props.items.length - 1}
      onNavigate={props.onNavigate}
      onClose={() => props.onClose(currentItem.root_id)}
    >
      {currentItem.kind === 'collection' ? (
        <GroupQuickLookContent
          groupId={currentItem.root_id}
          currentIndex={displayedIndex}
          totalCount={totalCount}
          onNavigate={props.onNavigate}
          onClose={() => props.onClose(currentItem.root_id)}
        />
      ) : (
        <QuickLookContent
          {...props}
          currentIndex={displayedIndex}
          thumbnailReady={decodedThumbnailItemId === currentItem.root_id}
          onReady={markMediaReady}
        />
      )}
    </QuickLookHost>
  );
}
