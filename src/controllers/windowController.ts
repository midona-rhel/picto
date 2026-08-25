import { openDetailWindow } from '../platform/shellApi';

export const windowController = {
  openDetailWindow(input: {
    hash: string;
    width?: number | null;
    height?: number | null;
  }): Promise<void> {
    return openDetailWindow(input);
  },

  closeCurrentWindow(): Promise<void> {
    return (window as any).picto.api.window.call('close');
  },

  setCurrentWindowAlwaysOnTop(value: boolean): Promise<void> {
    return (window as any).picto.api.window.call('setAlwaysOnTop', { value });
  },

  captureCurrentWindowRect(rect: { x: number; y: number; width: number; height: number }): Promise<string> {
    return (window as any).picto.api.window.call('captureRect', rect);
  },
};
