import { ActionIcon } from '@mantine/core';
import { IconX } from '@tabler/icons-react';
import { VideoPlayer } from './video/VideoPlayer';
import { ZoomableImage } from './ZoomableImage';

interface ZoomOverlayProps {
  url: string;
  alt: string;
  isVideo: boolean;
  onClose: () => void;
  animationName?: string;
  hash?: string;
}

export function ZoomOverlay({ url, alt, isVideo, onClose, animationName = 'zoomExpand', hash }: ZoomOverlayProps) {
  return (
    <div
      onClick={onClose}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 1000,
        backgroundColor: 'var(--color-black-99)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'default',
        animation: `${animationName} 0.15s ease-out forwards`,
      }}
    >
      <style>{`
        @keyframes ${animationName} {
          from { opacity: 0; transform: scale(0.9); }
          to { opacity: 1; transform: scale(1); }
        }
      `}</style>
      <ActionIcon
        variant="subtle"
        color="gray"
        size="lg"
        onClick={(e) => { e.stopPropagation(); onClose(); }}
        style={{
          position: 'absolute',
          top: 12,
          right: 12,
          zIndex: 1001,
        }}
        aria-label="Close zoom view"
      >
        <IconX size={20} color="var(--color-text-primary)" />
      </ActionIcon>
      {isVideo ? (
        <VideoPlayer
          src={url}
          muted
        />
      ) : (
        <ZoomableImage src={url} alt={alt} hash={hash} />
      )}
    </div>
  );
}
