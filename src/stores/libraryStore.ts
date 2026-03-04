import { create } from 'zustand';
import { api, libraryHost } from '#desktop/api';

export interface LibraryInfo {
  path: string;
  name: string;
  isCurrent: boolean;
  isPinned: boolean;
  exists: boolean;
}

interface LibraryState {
  libraries: LibraryInfo[];
  currentPath: string | null;
  switching: boolean;

  loadConfig: () => Promise<void>;
  switchLibrary: (path: string) => Promise<void>;
  createLibrary: (name: string, savePath: string) => Promise<void>;
  openLibrary: () => Promise<void>;
  removeLibrary: (path: string) => Promise<void>;
  deleteLibrary: (path: string) => Promise<void>;
  togglePin: (path: string) => Promise<void>;
  renameLibrary: (path: string, newName: string) => Promise<void>;
  relocateLibrary: (oldPath: string) => Promise<void>;
  getLibraryInfo: () => Promise<{ path: string; name: string; file_count: number } | null>;
  setSwitching: (value: boolean) => void;
}

function libraryDisplayName(libPath: string): string {
  const base = libPath.split('/').pop() ?? libPath;
  return base.endsWith('.library') ? base.slice(0, -8) : base;
}

export const useLibraryStore = create<LibraryState>((set) => ({
  libraries: [],
  currentPath: null,
  switching: false,

  loadConfig: async () => {
    const config = await libraryHost.getConfig();
    const existsMap = config.existsMap ?? {};
    const libraries: LibraryInfo[] = (config.libraryHistory ?? []).map((p) => ({
      path: p,
      name: libraryDisplayName(p),
      isCurrent: p === config.currentPath,
      isPinned: (config.pinnedLibraries ?? []).includes(p),
      exists: existsMap[p] ?? true,
    }));
    set({ libraries, currentPath: config.currentPath });
  },

  switchLibrary: async (path) => {
    await libraryHost.switch(path);
  },

  createLibrary: async (name, savePath) => {
    await libraryHost.create(name, savePath);
  },

  openLibrary: async () => {
    await libraryHost.open();
  },

  removeLibrary: async (path) => {
    await libraryHost.remove(path);
    await useLibraryStore.getState().loadConfig();
  },

  deleteLibrary: async (path) => {
    await libraryHost.delete(path);
    await useLibraryStore.getState().loadConfig();
  },

  togglePin: async (path) => {
    await libraryHost.togglePin(path);
    await useLibraryStore.getState().loadConfig();
  },

  renameLibrary: async (path, newName) => {
    await libraryHost.rename(path, newName);
    await useLibraryStore.getState().loadConfig();
  },

  relocateLibrary: async (oldPath) => {
    await libraryHost.relocate(oldPath);
    await useLibraryStore.getState().loadConfig();
  },

  getLibraryInfo: async () => {
    try {
      return await api.library.getInfo();
    } catch {
      return null;
    }
  },

  setSwitching: (value) => set({ switching: value }),
}));
