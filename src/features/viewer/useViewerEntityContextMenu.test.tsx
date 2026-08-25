import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { MenuItem } from '../../shared/ui/ContextMenu';
import { filesController } from '../../controllers/filesController';
import { buildFlashPlaybackContextEntries, useViewerEntityContextMenu } from './useViewerEntityContextMenu';

function CaptureMenuHarness({ capture }: { capture: () => Promise<string> }) {
  const menu = useViewerEntityContextMenu({ hash: 'flash-hash', captureCurrentFrame: capture });
  return <><button onContextMenu={menu.open}>Open</button>{menu.menu}</>;
}

function ImageMenuHarness() {
  const menu = useViewerEntityContextMenu({
    hash: 'image-hash',
    name: 'Example',
    mime: 'image/png',
  });
  return <><button onContextMenu={menu.open}>Open</button>{menu.menu}</>;
}

function LibraryImageMenuHarness() {
  const menu = useViewerEntityContextMenu({
    hash: 'image-hash',
    itemId: 42,
    kind: 'media',
    lifecycle: 'active',
    name: 'Example',
    mime: 'image/png',
  });
  return <><button onContextMenu={menu.open}>Open</button>{menu.menu}</>;
}

describe('Flash viewer context menu', () => {
  it('uses the live playback controller for play, stop, and volume actions', () => {
    const togglePlay = vi.fn();
    const stop = vi.fn();
    const toggleMute = vi.fn();
    const entries = buildFlashPlaybackContextEntries({
      isPlaying: true,
      muted: false,
      volume: 0.8,
      togglePlay,
      stop,
      toggleMute,
      setVolume: vi.fn(),
    }) as MenuItem[];

    expect(entries.map((entry) => entry.label)).toEqual(['Pause', 'Stop', 'Mute']);
    entries.forEach((entry) => entry.action());
    expect(togglePlay).toHaveBeenCalledOnce();
    expect(stop).toHaveBeenCalledOnce();
    expect(toggleMute).toHaveBeenCalledOnce();
  });

  it('closes the menu and stores one frame supplied by the isolated capture owner', async () => {
    const setThumbnail = vi.spyOn(filesController, 'setThumbnail').mockResolvedValue(undefined);
    const capture = vi.fn(async () => 'data:image/png;base64,frame');
    render(<CaptureMenuHarness capture={capture} />);

    fireEvent.contextMenu(screen.getByRole('button', { name: 'Open' }), { clientX: 20, clientY: 20 });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Set as Thumbnail' }));

    await waitFor(() => expect(setThumbnail).toHaveBeenCalledWith('flash-hash', 'data:image/png;base64,frame'));
    expect(capture).toHaveBeenCalledOnce();
  });
});

describe('Image viewer context menu', () => {
  it('uses shared physical-file actions without grid display controls', async () => {
    render(<ImageMenuHarness />);

    fireEvent.contextMenu(screen.getByRole('button', { name: 'Open' }), { clientX: 20, clientY: 20 });
    const labels = (await screen.findAllByRole('menuitem')).map((entry) => entry.textContent ?? '');
    expect(labels.some((label) => label.startsWith('Copy'))).toBe(true);
    expect(labels.some((label) => label.startsWith('Copy File Path'))).toBe(true);
    expect(labels.some((label) => label.startsWith('Copy Name'))).toBe(true);
    expect(labels.some((label) => label.startsWith('Copy as Link'))).toBe(true);
    expect(screen.queryByRole('menuitem', { name: 'View in Grayscale' })).not.toBeInTheDocument();
  });

  it('reuses library item actions while omitting grid-only display and selection controls', async () => {
    render(<LibraryImageMenuHarness />);

    fireEvent.contextMenu(screen.getByRole('button', { name: 'Open' }), { clientX: 20, clientY: 20 });
    expect(await screen.findByRole('menuitem', { name: /^Add to Folder/ })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /^Add Tags/ })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /^Auto Tag/ })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /^Export\.\.\./ })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /^Regenerate Thumbnail/ })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: /^Move to Trash/ })).toBeInTheDocument();
    expect(screen.queryByRole('menuitem', { name: 'Select All' })).not.toBeInTheDocument();
    expect(screen.queryByRole('menuitem', { name: 'View in Grayscale' })).not.toBeInTheDocument();
  });
});
