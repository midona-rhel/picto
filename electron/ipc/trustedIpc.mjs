export function createTrustedIpcHandle(ipcMain, ownsWebContents) {
  return (channel, listener) => ipcMain.handle(channel, (event, ...args) => {
    if (!ownsWebContents(event.sender)) {
      throw new Error(`Rejected IPC from an untrusted renderer: ${channel}`);
    }
    return listener(event, ...args);
  });
}
