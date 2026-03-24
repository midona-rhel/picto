import type { RefObject } from 'react';
import { computeTextHeight, TEXT_NAME_ROW_H } from '../gridLayout';
import type { MediaItem } from '../shared';
import type { LayoutItem } from '../layoutMath';

export function GridInlineRenameOverlay(args: {
  renamingHash: string;
  positions: LayoutItem[];
  images: MediaItem[];
  showTileName: boolean;
  showResolution: boolean;
  scrollRoot: HTMLDivElement | null;
  renameInputRef: RefObject<HTMLInputElement>;
  renameValue: string;
  setRenameValue: (value: string) => void;
  commitRename: () => void;
  cancelRename: () => void;
}) {
  const {
    renamingHash,
    positions,
    images,
    showTileName,
    showResolution,
    scrollRoot,
    renameInputRef,
    renameValue,
    setRenameValue,
    commitRename,
    cancelRename,
  } = args;

  const idx = images.findIndex((i) => i.hash === renamingHash);
  const pos = idx >= 0 ? positions[idx] : null;
  if (!pos) return null;

  const textHeight = computeTextHeight(showTileName, showResolution);
  const imageHeight = pos.h - textHeight;
  const canvasRoot = scrollRoot?.querySelector<HTMLElement>('[data-canvas-grid-root]');
  const offsetTop = canvasRoot?.offsetTop ?? 0;

  return (
    <input
      ref={renameInputRef}
      value={renameValue}
      onChange={(e) => setRenameValue(e.target.value)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') commitRename();
        if (e.key === 'Escape') cancelRename();
      }}
      onBlur={commitRename}
      style={{
        position: 'absolute',
        top: offsetTop + pos.y + imageHeight,
        left: pos.x,
        width: pos.w,
        height: TEXT_NAME_ROW_H,
        fontSize: 'var(--font-size-md)',
        lineHeight: '1',
        textAlign: 'center',
        padding: '0 4px',
        border: '1px solid var(--color-primary)',
        borderRadius: 3,
        background: 'var(--color-bg-primary, #1e1e1e)',
        color: 'var(--color-text-primary)',
        outline: 'none',
        boxSizing: 'border-box',
        zIndex: 10,
        fontFamily: 'var(--font-family)',
      }}
    />
  );
}
