import { api, libraryHost } from '#desktop/api';
import type { LibraryInfo } from '../shared/types/api';
import type { LibraryConfig } from '../platform/nativeIntegration';

export const libraryController = {
  getInfo(): Promise<LibraryInfo> {
    return api.library.getInfo();
  },

  wipeImageData() {
    return api.library.wipeImageData();
  },

  getConfig(): Promise<LibraryConfig> {
    return libraryHost.getConfig();
  },

  create(name: string, savePath: string): Promise<void> {
    return libraryHost.create(name, savePath);
  },

  open(): Promise<void> {
    return libraryHost.open();
  },

  switch(path: string): Promise<void> {
    return libraryHost.switch(path);
  },

  remove(path: string): Promise<void> {
    return libraryHost.remove(path);
  },

  delete(path: string): Promise<void> {
    return libraryHost.delete(path);
  },

  togglePin(path: string): Promise<void> {
    return libraryHost.togglePin(path);
  },

  rename(path: string, newName: string): Promise<void> {
    return libraryHost.rename(path, newName);
  },

  relocate(oldPath: string): Promise<void> {
    return libraryHost.relocate(oldPath);
  },

  setMeta(path: string, meta: { icon?: string | null; color?: string | null }): Promise<void> {
    return libraryHost.setMeta(path, meta);
  },
};
