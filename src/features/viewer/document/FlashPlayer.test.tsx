import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FlashPlayer } from './FlashPlayer';
import { fitFlashStage } from './flashStageGeometry';
import type { CurrentFrameCapture } from '../currentFrameCapture';
import { windowController } from '../../../controllers/windowController';
import { registerShortcutScope, resetShortcutRuntimeForTests } from '../../../runtime/shortcutRuntime';

function FlashPlayerHarness({ onFrameCaptureChange }: { onFrameCaptureChange?: (capture: CurrentFrameCapture | null) => void } = {}) {
  return (
    <FlashPlayer
      src="media://localhost/file/example.swf"
      onFrameCaptureChange={onFrameCaptureChange}
    />
  );
}

describe('FlashPlayer', () => {
  beforeEach(() => {
    resetShortcutRuntimeForTests();
    document.head.querySelectorAll('script[data-picto-ruffle]').forEach((script) => script.remove());
    delete window.RufflePlayer;
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('fits the native Flash stage inside the available viewer without changing its aspect ratio', () => {
    expect(fitFlashStage({ width: 1200, height: 600 }, { width: 400, height: 800 })).toEqual({
      width: 300,
      height: 600,
    });
    expect(fitFlashStage({ width: 600, height: 900 }, { width: 800, height: 400 })).toEqual({
      width: 600,
      height: 300,
    });
  });

  it('loads the SWF through Ruffle with network and script access constrained', async () => {
    const load = vi.fn().mockResolvedValue(undefined);
    let suspended = false;
    const runtime = {
      load,
      readyState: 0,
      metadata: { width: 400, height: 800 },
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

    class TestResizeObserver {
      constructor(private readonly callback: ResizeObserverCallback) {}
      observe() {
        this.callback([{ contentRect: { width: 1200, height: 600 } } as ResizeObserverEntry], this as unknown as ResizeObserver);
      }
      disconnect() {}
      unobserve() {}
    }
    vi.stubGlobal('ResizeObserver', TestResizeObserver);

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
    expect(document.querySelector('[data-flash-stage]')).toHaveStyle({ width: '300px', height: '600px' });
    expect(screen.getByRole('button', { name: 'Pause Flash content' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Stop Flash content' })).toBeVisible();
    expect(document.querySelector('[data-flash-player]')).toContainElement(document.querySelector('[data-flash-controls]'));
    expect(document.querySelector('[data-media-controls]')).toHaveAttribute('data-visible', 'true');

    fireEvent.keyDown(window, { key: 'k' });
    expect(runtime.suspend).toHaveBeenCalledOnce();
    expect(screen.getByRole('button', { name: 'Play Flash content' })).toBeVisible();
    fireEvent.keyDown(window, { key: 'k' });
    expect(runtime.resume).toHaveBeenCalledOnce();
    expect(screen.getByRole('button', { name: 'Pause Flash content' })).toBeVisible();

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
    const stage = document.querySelector<HTMLElement>('[data-flash-stage]');
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
