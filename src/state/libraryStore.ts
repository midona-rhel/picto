import { create } from 'zustand';
import { libraryController } from '../controllers/libraryController';

export interface LibraryInfo {
  path: string;
  name: string;
  isCurrent: boolean;
  isPinned: boolean;
  exists: boolean;
  icon: string | null;
  color: string | null;
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
  setLibraryIcon: (path: string, icon: string | null) => Promise<void>;
  setLibraryColor: (path: string, color: string | null) => Promise<void>;
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
    const config = await libraryController.getConfig();
    const existsMap = config.existsMap ?? {};
    const metaMap = config.libraryMeta ?? {};
    const libraries: LibraryInfo[] = (config.libraryHistory ?? []).map((p) => ({
      path: p,
      name: libraryDisplayName(p),
      isCurrent: p === config.currentPath,
      isPinned: (config.pinnedLibraries ?? []).includes(p),
      exists: existsMap[p] ?? true,
      icon: metaMap[p]?.icon ?? null,
      color: metaMap[p]?.color ?? null,
    }));
    set({ libraries, currentPath: config.currentPath });
  },

  switchLibrary: async (path) => {
    await libraryController.switch(path);
  },

  createLibrary: async (name, savePath) => {
    await libraryController.create(name, savePath);
  },

  openLibrary: async () => {
    await libraryController.open();
  },

  removeLibrary: async (path) => {
    await libraryController.remove(path);
    await useLibraryStore.getState().loadConfig();
  },

  deleteLibrary: async (path) => {
    await libraryController.delete(path);
    await useLibraryStore.getState().loadConfig();
  },

  togglePin: async (path) => {
    await libraryController.togglePin(path);
    await useLibraryStore.getState().loadConfig();
  },

  renameLibrary: async (path, newName) => {
    await libraryController.rename(path, newName);
    await useLibraryStore.getState().loadConfig();
  },

  relocateLibrary: async (oldPath) => {
    await libraryController.relocate(oldPath);
    await useLibraryStore.getState().loadConfig();
  },

  setLibraryIcon: async (path, icon) => {
    await libraryController.setMeta(path, { icon });
    await useLibraryStore.getState().loadConfig();
  },

  setLibraryColor: async (path, color) => {
    await libraryController.setMeta(path, { color });
    await useLibraryStore.getState().loadConfig();
  },

  getLibraryInfo: async () => {
    try {
      return await libraryController.getInfo();
    } catch {
      return null;
    }
  },

  setSwitching: (value) => set({ switching: value }),
}));
