import { describe, expect, it, vi } from 'vitest';
import { createTrustedIpcHandle } from './trustedIpc.mjs';

describe('trusted IPC registration', () => {
  it('runs handlers only for renderer contents owned by Picto', async () => {
    const registrations = new Map();
    const ipcMain = {
      handle: vi.fn((channel, listener) => registrations.set(channel, listener)),
    };
    const trustedSender = { id: 1 };
    const listener = vi.fn((_event, value) => value * 2);
    const handle = createTrustedIpcHandle(ipcMain, (sender) => sender === trustedSender);

    handle('picto:test', listener);
    const registered = registrations.get('picto:test');

    expect(registered({ sender: trustedSender }, 4)).toBe(8);
    expect(listener).toHaveBeenCalledOnce();
    expect(() => registered({ sender: { id: 2 } }, 4)).toThrow('untrusted renderer');
    expect(listener).toHaveBeenCalledOnce();
  });
});
