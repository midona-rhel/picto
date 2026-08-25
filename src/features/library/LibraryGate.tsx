import { Fragment, type ReactNode, useEffect, useRef, useState } from 'react';
import { IconBooks } from '@tabler/icons-react';
import { invoke, listen } from '../../platform/ipc';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { WindowControls } from '../../shared/ui/WindowControls';
import styles from './LibraryGate.module.css';

interface LibraryConfig {
  currentPath: string | null;
  openingPath?: string | null;
}

interface CloudSyncStatus {
  phase: string;
  completed_units: number;
  total_units: number | null;
  message: string;
}

type LibraryState =
  | { kind: 'loading'; path?: string }
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
  const [cloudStatus, setCloudStatus] = useState<CloudSyncStatus | null>(null);
  const managerRequested = useRef(false);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    void listen<{ path: string }>('library-switched', ({ payload }) => {
      if (!cancelled) setLibrary({ kind: 'open', path: payload.path });
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisteners.push(dispose);
    });
    void listen<{ path: string }>('library-switching', ({ payload }) => {
      if (!cancelled) setLibrary({ kind: 'loading', path: payload.path });
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisteners.push(dispose);
    });

    void getLibraryConfig()
      .then((config) => {
        if (cancelled) return;
        setLibrary(config.openingPath
          ? { kind: 'loading', path: config.openingPath }
          : config.currentPath
            ? { kind: 'open', path: config.currentPath }
            : { kind: 'closed' });
      })
      .catch(() => {
        if (!cancelled) setLibrary({ kind: 'closed' });
      });

    return () => {
      cancelled = true;
      unlisteners.forEach((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    if (library.kind !== 'loading' || !library.path) {
      setCloudStatus(null);
      return;
    }
    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const status = await invoke<CloudSyncStatus>('cloud.status.get');
        if (!cancelled) setCloudStatus(status);
      } catch {
        // The native state may still be opening; the next poll retries.
      }
      if (!cancelled) timeout = setTimeout(poll, 100);
    };
    void poll();
    return () => {
      cancelled = true;
      if (timeout) clearTimeout(timeout);
    };
  }, [library]);

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
      <div className={styles.titlebar} data-window-drag-region="">
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
      ) : library.path ? (
        <main className={styles.content} aria-live="polite">
          <span className={styles.spinner} aria-hidden="true" />
          <h1>{cloudStatus?.message || 'Opening your library'}</h1>
          <div className={styles.progress}>
            <ProgressBar
              done={cloudStatus?.completed_units ?? 0}
              total={cloudStatus?.total_units ?? 0}
              indeterminate={!cloudStatus?.total_units}
              height={3}
            />
          </div>
        </main>
      ) : null}
    </div>
  );
}
