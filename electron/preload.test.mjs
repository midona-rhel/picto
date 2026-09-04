import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { runInNewContext } from 'node:vm';
import { expect, test, vi } from 'vitest';

test.each([true, false])('applies the canonical startup theme before page scripts (root already exists: %s)', (rootExists) => {
  const root = { dataset: {}, style: {} };
  const document = { documentElement: rootExists ? root : null };
  let onMutation;
  const disconnect = vi.fn();
  const theme = { requested: 'auto', applied: 'light', colorScheme: 'light', backgroundColor: '#ffffff', platform: 'linux' };
  const bridge = vi.fn();
  runInNewContext(readFileSync(resolve(process.cwd(), 'electron/preload.cjs'), 'utf8'), {
    require: () => ({
      ipcRenderer: { sendSync: (channel) => { expect(channel).toBe('picto:startup-theme'); return theme; } },
      contextBridge: { exposeInMainWorld: bridge },
    }),
    document,
    MutationObserver: class {
      constructor(callback) { onMutation = callback; }
      observe() {}
      disconnect = disconnect;
    },
    process: { env: {} },
  });
  if (!rootExists) {
    document.documentElement = root;
    onMutation();
    expect(disconnect).toHaveBeenCalledOnce();
  }
  expect(root.dataset).toEqual({ theme: 'light', mantineColorScheme: 'light', platform: 'linux' });
  expect(root.style).toEqual({ colorScheme: 'light', backgroundColor: '#ffffff' });
  expect(bridge).toHaveBeenCalledWith('picto', expect.objectContaining({ startupTheme: theme }));
});
