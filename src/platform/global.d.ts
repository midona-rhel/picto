export {};

declare global {
  interface Window {
    picto?: {
      api: {
        invoke: <T = unknown>(command: string, args?: Record<string, unknown>) => Promise<T>;
        window?: {
          call?: (method: string, payload?: Record<string, unknown>) => Promise<unknown>;
        };
        popupMenu?: () => Promise<void>;
      };
      events: {
        on<T = unknown>(name: string, handler: (payload: T) => void): Promise<() => void>;
        emit<T = unknown>(name: string, payload: T): Promise<void>;
        emitTo<T = unknown>(target: string, name: string, payload: T): Promise<void>;
      };
      dialog?: {
        open: (opts?: Record<string, unknown>) => Promise<string | string[] | null>;
        save: (opts?: Record<string, unknown>) => Promise<string | null>;
      };
      clipboard?: {
        writeText: (text: string) => Promise<void>;
        copyFile?: (filePath: string) => Promise<void>;
        copyImage?: (filePath: string) => Promise<void>;
      };
      monitor?: {
        current: () => Promise<{ scaleFactor: number; size: { width: number; height: number } }>;
      };
      webview?: {
        onDragDropEvent: (handler: (event: { payload: unknown }) => void) => Promise<() => void>;
        startNativeDrag: (hashes: string[], iconDataUrl?: string | null) => Promise<{ ok: boolean } | null>;
      };
      search?: {
        reverseImage: (filePath: string, engine: string) => Promise<string>;
      };
      library?: {
        create: (name: string, savePath: string) => Promise<string>;
        open: () => Promise<string | null>;
        switch: (path: string) => Promise<void>;
        remove: (path: string) => Promise<void>;
        delete: (path: string) => Promise<{ deleted: boolean }>;
        togglePin: (path: string) => Promise<void>;
        getConfig: () => Promise<{
          libraryHistory: string[];
          pinnedLibraries: string[];
          lastLibrary: string | null;
          currentPath: string | null;
          existsMap: Record<string, boolean>;
        }>;
        rename: (path: string, newName: string) => Promise<{ newPath: string }>;
        relocate: (oldPath: string) => Promise<{ action: string; newPath?: string }>;
      };
    };
  }
}
