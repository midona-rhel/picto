import { requireDesktop } from './ipc';
import type { UnlistenFn } from './ipc';

export class PhysicalSize {
  width: number;
  height: number;

  constructor(width: number, height: number) {
    this.width = width;
    this.height = height;
  }
}

class DesktopWindow {
  async show(): Promise<void> {
    await requireDesktop().api.window?.call?.('show');
  }

  async setTheme(theme: string): Promise<void> {
    await requireDesktop().api.window?.call?.('setTheme', { theme });
  }

  async startDragging(): Promise<void> {
    await requireDesktop().api.window?.call?.('startDragging');
  }

  async minimize(): Promise<void> {
    await requireDesktop().api.window?.call?.('minimize');
  }

  async toggleMaximize(): Promise<void> {
    await requireDesktop().api.window?.call?.('toggleMaximize');
  }

  async setSize(size: PhysicalSize): Promise<void> {
    await requireDesktop().api.window?.call?.('setSize', { width: size.width, height: size.height });
  }

  async setAlwaysOnTop(value: boolean): Promise<void> {
    await requireDesktop().api.window?.call?.('setAlwaysOnTop', { value });
  }

  async close(): Promise<void> {
    await requireDesktop().api.window?.call?.('close');
  }

  async setFocus(): Promise<void> {
    await requireDesktop().api.window?.call?.('setFocus');
  }

  async isMaximized(): Promise<boolean> {
    return Boolean(await requireDesktop().api.window?.call?.('isMaximized'));
  }

  async outerPosition(): Promise<{ x: number; y: number }> {
    const value = await requireDesktop().api.window?.call?.('outerPosition');
    return (value as { x: number; y: number }) ?? { x: 0, y: 0 };
  }

  async innerSize(): Promise<{ width: number; height: number }> {
    const value = await requireDesktop().api.window?.call?.('innerSize');
    return (value as { width: number; height: number }) ?? { width: window.innerWidth, height: window.innerHeight };
  }

  async onResized(handler: (event: { payload: { width: number; height: number } }) => void): Promise<UnlistenFn> {
    return requireDesktop().events.on('picto:window-resized', (payload) => {
      handler({ payload: payload as { width: number; height: number } });
    });
  }

  async onMoved(handler: () => void): Promise<UnlistenFn> {
    return requireDesktop().events.on('picto:window-moved', () => handler());
  }
}

export function getCurrentWindow(): DesktopWindow {
  return new DesktopWindow();
}

export async function setTheme(theme: string): Promise<void> {
  await requireDesktop().api.window?.call?.('setTheme', { theme });
}

export function getCurrentWebview() {
  return {
    onDragDropEvent: (handler: (event: { payload: unknown }) => void) => {
      if (!window.picto?.webview?.onDragDropEvent) return Promise.resolve(() => {});
      return window.picto.webview.onDragDropEvent(handler);
    },
    startNativeDrag: (hashes: string[], iconDataUrl?: string | null) => {
      if (!window.picto?.webview?.startNativeDrag) return Promise.resolve(null);
      return window.picto.webview.startNativeDrag(hashes, iconDataUrl);
    },
  };
}
