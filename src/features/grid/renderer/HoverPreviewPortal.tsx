import { createPortal } from 'react-dom';
import { mediaFileUrl } from '../../../shared/lib/mediaUrl';
import {
  type HoverPreviewData,
  useHoverPreviewLoaded,
} from './useCanvasHoverInteractions';

const PREVIEW_INSET = 48;

export function HoverPreviewPortal({ hash, mime }: HoverPreviewData) {
  const fullUrl = mediaFileUrl(hash, mime);
  const { loaded, markLoaded } = useHoverPreviewLoaded(fullUrl);

  return createPortal(
    <div
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 200002,
        pointerEvents: 'none',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        backgroundColor: loaded ? 'rgba(0,0,0,0.6)' : 'transparent',
        transition: 'background-color 150ms ease',
      }}
    >
      <img
        src={fullUrl}
        alt=""
        onLoad={() => {
          markLoaded();
        }}
        style={{
          display: 'block',
          maxWidth: `calc(100vw - ${PREVIEW_INSET * 2}px)`,
          maxHeight: `calc(100vh - ${PREVIEW_INSET * 2}px)`,
          objectFit: 'contain',
          borderRadius: 8,
          boxShadow: '0 8px 48px rgba(0,0,0,0.7)',
          opacity: loaded ? 1 : 0,
          transition: 'opacity 150ms ease',
        }}
      />
    </div>,
    document.body,
  );
}
