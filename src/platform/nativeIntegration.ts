export async function open(options?: Record<string, unknown>): Promise<string | string[] | null> {
  return window.picto?.dialog?.open ? window.picto.dialog.open(options) : null;
}

export async function save(options?: Record<string, unknown>): Promise<string | null> {
  return window.picto?.dialog?.save ? window.picto.dialog.save(options) : null;
}

export async function writeText(text: string): Promise<void> {
  if (window.picto?.clipboard?.writeText) {
    await window.picto.clipboard.writeText(text);
    return;
  }
  await navigator.clipboard.writeText(text);
}

export async function copyFileToClipboard(filePath: string): Promise<void> {
  if (window.picto?.clipboard?.copyFile) {
    await window.picto.clipboard.copyFile(filePath);
    return;
  }
  // Fallback: copy path as text
  await navigator.clipboard.writeText(filePath);
}

export async function copyImageToClipboard(filePath: string): Promise<void> {
  if (window.picto?.clipboard?.copyImage) {
    await window.picto.clipboard.copyImage(filePath);
    return;
  }
  throw new Error('Image clipboard not available outside Electron');
}

export type ReverseImageEngine = 'tineye' | 'saucenao' | 'yandex' | 'sogou' | 'bing';

export async function reverseImageSearch(filePath: string, engine: ReverseImageEngine): Promise<string> {
  if (window.picto?.search?.reverseImage) {
    return window.picto.search.reverseImage(filePath, engine);
  }
  throw new Error('Reverse image search not available outside Electron');
}

export interface LibraryConfig {
  currentPath: string | null;
  libraryHistory: string[];
  pinnedLibraries: string[];
  existsMap: Record<string, boolean>;
}

export const libraryHost = {
  getConfig: async (): Promise<LibraryConfig> =>
    await window.picto?.library?.getConfig?.() ?? { currentPath: null, libraryHistory: [], pinnedLibraries: [], existsMap: {} },
  create: async (name: string, savePath: string): Promise<void> => {
    await window.picto?.library?.create?.(name, savePath);
  },
  open: async (): Promise<void> => {
    await window.picto?.library?.open?.();
  },
  switch: async (path: string): Promise<void> => {
    await window.picto?.library?.switch?.(path);
  },
  remove: async (path: string): Promise<void> => {
    await window.picto?.library?.remove?.(path);
  },
  delete: async (path: string): Promise<void> => {
    await window.picto?.library?.delete?.(path);
  },
  togglePin: async (path: string): Promise<void> => {
    await window.picto?.library?.togglePin?.(path);
  },
  rename: async (path: string, newName: string): Promise<void> => {
    await window.picto?.library?.rename?.(path, newName);
  },
  relocate: async (oldPath: string): Promise<void> => {
    await window.picto?.library?.relocate?.(oldPath);
  },
};
