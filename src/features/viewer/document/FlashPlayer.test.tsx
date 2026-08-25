import { useState } from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { FlashControls } from './FlashControls';
import { FlashPlayer, type FlashPlaybackController } from './FlashPlayer';
import type { CurrentFrameCapture } from '../currentFrameCapture';
import { windowController } from '../../../controllers/windowController';
import { registerShortcutScope, resetShortcutRuntimeForTests } from '../../../runtime/shortcutRuntime';

function FlashPlayerHarness({ onFrameCaptureChange }: { onFrameCaptureChange?: (capture: CurrentFrameCapture | null) => void } = {}) {
  const [controller, setController] = useState<FlashPlaybackController | null>(null);
  return (
    <>
      <FlashPlayer
        src="media://localhost/file/example.swf"
        onPlaybackChange={setController}
        onFrameCaptureChange={onFrameCaptureChange}
      />
      <FlashControls controller={controller} />
    </>
  );
}

describe('FlashPlayer', () => {
  beforeEach(() => {
    resetShortcutRuntimeForTests();
    document.head.querySelectorAll('script[data-picto-ruffle]').forEach((script) => script.remove());
    delete window.RufflePlayer;
  });

  it('loads the SWF through Ruffle with network and script access constrained', async () => {
    const load = vi.fn().mockResolvedValue(undefined);
    let suspended = false;
    const runtime = {
      load,
      readyState: 0,
      metadata: null,
      get suspended() { return suspended; },
      suspend: vi.fn(() => { suspended = true; }),
      resume: vi.fn(() => { suspended = false; }),
      volume: 1,
    };
    const player = Object.assign(document.createElement('div'), {
      ruffle: (version: number) => {
        expect(version).toBe(1);
        return runtime;
      },
    });
    const shadow = player.attachShadow({ mode: 'open' });
    window.RufflePlayer = {
      newest: () => ({ createPlayer: () => player as any }),
    };

    render(
      <MantineProvider>
        <FlashPlayerHarness />
      </MantineProvider>,
    );

    await waitFor(() => expect(load).toHaveBeenCalledWith({
      url: 'media://localhost/file/example.swf',
      autoplay: 'on',
      unmuteOverlay: 'hidden',
      allowScriptAccess: false,
      allowNetworking: 'internal',
      openUrlMode: 'confirm',
      contextMenu: 'off',
      showSwfDownload: false,
      allowFullscreen: false,
      menu: false,
      preloader: false,
      splashScreen: false,
    }));
    expect(shadow.querySelector('[data-picto-ruffle-chrome]')).toHaveTextContent('#play-button');

    await act(async () => { player.dispatchEvent(new Event('loadeddata')); });
    await waitFor(() => expect(document.querySelector('[data-flash-player]')).toHaveAttribute('data-status', 'ready'));
    expect(screen.getByRole('button', { name: 'Pause Flash content' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Stop Flash content' })).toBeVisible();
    expect(document.querySelector('[data-flash-player]')).not.toContainElement(document.querySelector('[data-flash-controls]'));

    const appShortcut = vi.fn(() => true);
    const escapedKey = vi.fn();
    const unregisterShortcut = registerShortcutScope(appShortcut);
    const stage = document.querySelector<HTMLElement>('[data-flash-player]')!;
    fireEvent.pointerDown(stage);
    window.addEventListener('keydown', escapedKey);
    fireEvent.keyDown(player, { key: 'ArrowLeft' });
    expect(appShortcut).not.toHaveBeenCalled();
    expect(escapedKey).toHaveBeenCalledOnce();
    window.removeEventListener('keydown', escapedKey);
    unregisterShortcut();

    fireEvent.click(screen.getByRole('button', { name: 'Pause Flash content' }));
    expect(runtime.suspend).toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Play Flash content' })).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Stop Flash content' }));
    expect(document.querySelector('[data-flash-player]')).toHaveAttribute('data-status', 'stopped');
    await waitFor(() => expect(load).toHaveBeenLastCalledWith(expect.objectContaining({ autoplay: 'off' })));

    fireEvent.click(screen.getByRole('button', { name: 'Play Flash content' }));
    expect(document.querySelector('[data-flash-player]')).toHaveAttribute('data-status', 'ready');
  });

  it('removes Flash controls from the painted frame before capturing the live stage', async () => {
    let capture: CurrentFrameCapture | null = null;
    const captureRect = vi.spyOn(windowController, 'captureCurrentWindowRect').mockImplementation(async () => {
      expect(document.querySelector('[data-flash-controls]')).toHaveStyle({ visibility: 'hidden' });
      return 'data:image/png;base64,frame';
    });
    render(
      <MantineProvider>
        <FlashPlayerHarness onFrameCaptureChange={(next) => { capture = next; }} />
      </MantineProvider>,
    );
    const stage = document.querySelector<HTMLElement>('[data-flash-player]');
    expect(stage).not.toBeNull();
    vi.spyOn(stage!, 'getBoundingClientRect').mockReturnValue({
      x: 10, y: 20, width: 300, height: 200,
      top: 20, right: 310, bottom: 220, left: 10,
      toJSON: () => ({}),
    });

    await waitFor(() => expect(capture).not.toBeNull());
    await expect(capture!()).resolves.toBe('data:image/png;base64,frame');
    expect(captureRect).toHaveBeenCalledWith({ x: 10, y: 20, width: 300, height: 200 });
    expect(document.querySelector('[data-flash-controls]')).not.toHaveStyle({ visibility: 'hidden' });
  });
});
