import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { mediaFileUrl } from '../../../shared/lib/mediaUrl';
import {
  type HoverPreviewData,
  useHoverPreviewLoaded,
} from './useCanvasPointerInteractions';

const PREVIEW_INSET = 48;
const MIN_WAIT_MS = 150;

export function HoverPreviewPortal({ hash, mime }: HoverPreviewData) {
  const fullUrl = mediaFileUrl(hash, mime);
  const { loaded: decoded, markLoaded } = useHoverPreviewLoaded(fullUrl);
  const [minWaitPassed, setMinWaitPassed] = useState(false);
  const mountTimeRef = useRef(performance.now());

  useEffect(() => {
    mountTimeRef.current = performance.now();
    setMinWaitPassed(false);
    const timer = setTimeout(() => setMinWaitPassed(true), MIN_WAIT_MS);
    return () => clearTimeout(timer);
  }, [fullUrl]);

  // Show only after both conditions are met
  const visible = decoded && minWaitPassed;

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
        backgroundColor: visible ? 'rgba(0,0,0,0.6)' : 'transparent',
        transition: 'background-color 150ms ease',
      }}
    >
      <img
        src={fullUrl}
        alt=""
        onLoad={markLoaded}
        style={{
          display: 'block',
          maxWidth: `calc(100vw - ${PREVIEW_INSET * 2}px)`,
          maxHeight: `calc(100vh - ${PREVIEW_INSET * 2}px)`,
          objectFit: 'contain',
          borderRadius: 8,
          boxShadow: '0 8px 48px rgba(0,0,0,0.7)',
          opacity: visible ? 1 : 0,
          transition: 'opacity 150ms ease',
        }}
      />
    </div>,
    document.body,
  );
}
