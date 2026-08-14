import { Fragment, type ReactNode, useEffect, useRef, useState } from 'react';
import { IconBooks } from '@tabler/icons-react';
import { invoke, listen } from '../../platform/ipc';
import { WindowControls } from '../../shared/ui/WindowControls';
import styles from './LibraryGate.module.css';

interface LibraryConfig {
  currentPath: string | null;
}

type LibraryState =
  | { kind: 'loading' }
  | { kind: 'closed' }
  | { kind: 'open'; path: string };

function getLibraryConfig(): Promise<LibraryConfig> {
  return (window as any).picto.library.getConfig();
}

function openLibraryManager(): Promise<void> {
  return invoke('open_library_manager');
}

export function LibraryGate({ children }: { children: ReactNode }) {
  const [library, setLibrary] = useState<LibraryState>({ kind: 'loading' });
  const managerRequested = useRef(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void listen<{ path: string }>('library-switched', ({ payload }) => {
      if (!cancelled) setLibrary({ kind: 'open', path: payload.path });
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });

    void getLibraryConfig()
      .then((config) => {
        if (cancelled) return;
        setLibrary(config.currentPath
          ? { kind: 'open', path: config.currentPath }
          : { kind: 'closed' });
      })
      .catch(() => {
        if (!cancelled) setLibrary({ kind: 'closed' });
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (library.kind !== 'closed' || managerRequested.current) return;
    managerRequested.current = true;
    void openLibraryManager().catch(() => {
      managerRequested.current = false;
    });
  }, [library.kind]);

  if (library.kind === 'open') return <Fragment key={library.path}>{children}</Fragment>;

  return (
    <div className={styles.root}>
      <div className={styles.titlebar}>
        <WindowControls />
      </div>
      {library.kind === 'closed' ? (
        <main className={styles.content}>
          <IconBooks size={30} stroke={1.25} />
          <h1>Open a library to start</h1>
          <p>Create a new Picto library or open one already on this device.</p>
          <button
            className={styles.action}
            type="button"
            onClick={() => void openLibraryManager()}
          >
            Choose Library…
          </button>
        </main>
      ) : null}
    </div>
  );
}
