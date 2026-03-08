import { DetailView } from '../../grid/DetailView';
import { QuickLook } from '../../grid/QuickLook';
import { Slideshow } from './Slideshow';
import type { ViewerHostController } from '../hooks/useViewerHost';

interface ViewerHostProps {
  viewer: ViewerHostController;
}

export function ViewerHost({ viewer }: ViewerHostProps) {
  const { mode, session, source } = viewer;
  if (!mode || !session || !source) return null;

  if (mode === 'slideshow') {
    return (
      <Slideshow
        images={source.images}
        startIndex={session.currentIndex}
        onClose={() => viewer.close(session.currentHash)}
      />
    );
  }

  if (mode === 'quick_look') {
    return (
      <QuickLook
        images={source.images}
        currentIndex={session.currentIndex}
        onNavigate={viewer.navigate}
        totalCount={source.totalCount}
        onClose={(exitHash) => viewer.close(exitHash)}
        onImageChange={(hash) => source.onQuickLookImageChange?.(hash)}
        onLoadMore={source.hasMore ? source.loadMore : undefined}
      />
    );
  }

  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        zIndex: 30,
        background: 'var(--color-background)',
      }}
    >
      <DetailView
        images={source.images}
        currentIndex={session.currentIndex}
        onNavigate={viewer.navigate}
        totalCount={source.totalCount}
        onClose={(exitHash) => viewer.close(exitHash)}
        onStateChange={(state, controls) => source.onDetailStateChange?.(state, controls)}
        onImageChange={(hash) => source.onDetailImageChange?.(hash)}
        onLoadMore={source.hasMore ? source.loadMore : undefined}
        inboxMode={source.inboxMode}
        onInboxAction={source.onInboxAction}
      />
    </div>
  );
}
