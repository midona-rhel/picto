/**
 * Hover preview portal — full-screen enlarged image overlay.
 *
 * Rendered as a React portal to document.body. Shows the full-size media
 * file with a dark backdrop. Fades in over 150ms once image is decoded.
 *
 * CSS matches legacy v0.5.0-alpha HoverPreviewPortal exactly.
 */

import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { mediaFileUrl } from '../../../shared/lib/mediaUrl';

const PREVIEW_INSET = 48;
const MIN_WAIT_MS = 150;

interface Props {
  fileHash: string;
  mime: string;
}

export function HoverPreviewPortal({ fileHash, mime }: Props) {
  const fullUrl = mediaFileUrl(fileHash, mime);
  const [decoded, setDecoded] = useState(false);
  const [minWaitPassed, setMinWaitPassed] = useState(false);

  // Always reset on URL change — no cache-driven shortcuts.
  // The user should experience the same fade-in every time.
  useEffect(() => {
    setDecoded(false);
    setMinWaitPassed(false);
    const timer = setTimeout(() => setMinWaitPassed(true), MIN_WAIT_MS);
    return () => clearTimeout(timer);
  }, [fullUrl]);

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
        onLoad={() => setDecoded(true)}
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
