import { useCallback, useEffect, useState } from 'react';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
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
  const [decodedThumbnailUrl, setDecodedThumbnailUrl] = useState<string | null>(null);
  const currentItem = props.items[displayedIndex] ?? null;
  const [mediaReady, setMediaReady] = useState(false);
  const markMediaReady = useCallback(() => setMediaReady(true), []);

  useEffect(() => {
    if (props.currentIndex === displayedIndex) return;
    const requestedItem = props.items[props.currentIndex] ?? null;
    if (!requestedItem) return;

    const isRequestedImage = requestedItem.kind !== 'collection'
      && detailRendererKind(requestedItem.mime) === 'image';
    if (!isRequestedImage) {
      setDecodedThumbnailItemId(null);
      setDecodedThumbnailUrl(null);
      setDisplayedIndex(props.currentIndex);
      return;
    }

    let cancelled = false;
    const image = new Image();
    const commit = (url: string) => {
      if (cancelled) return;
      setDecodedThumbnailItemId(requestedItem.root_id);
      setDecodedThumbnailUrl(url);
      setDisplayedIndex(props.currentIndex);
    };
    const thumbnailUrl = mediaThumbnailUrl(requestedItem.content_hash);
    image.onload = () => {
      if (typeof image.decode === 'function') {
        image.decode().then(() => commit(thumbnailUrl)).catch(() => commit(thumbnailUrl));
      } else {
        commit(thumbnailUrl);
      }
    };
    image.onerror = () => {
      const originalUrl = mediaFileUrl(requestedItem.content_hash, requestedItem.mime);
      const original = new Image();
      original.onload = () => {
        if (typeof original.decode === 'function') {
          original.decode().then(() => commit(originalUrl)).catch(() => commit(originalUrl));
        } else {
          commit(originalUrl);
        }
      };
      original.onerror = () => commit(thumbnailUrl);
      original.src = originalUrl;
    };
    image.src = thumbnailUrl;
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
          thumbnailUrlOverride={decodedThumbnailItemId === currentItem.root_id
            ? decodedThumbnailUrl ?? undefined
            : undefined}
          onReady={markMediaReady}
        />
      )}
    </QuickLookHost>
  );
}
