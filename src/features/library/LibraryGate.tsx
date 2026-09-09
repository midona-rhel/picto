import { Fragment, type ReactNode, useCallback, useEffect, useRef, useState } from 'react';
import { IconBooks } from '@tabler/icons-react';
import { invoke, listen } from '../../platform/ipc';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { WindowControls } from '../../shared/ui/WindowControls';
import styles from './LibraryGate.module.css';
import { t } from '../../i18n';
import { setRecentFoldersLibrary } from '../../shared/hooks/useRecentFolders';

interface LibraryConfig {
  currentPath: string | null;
  openingPath?: string | null;
}

interface LibraryOpenFailure {
  path?: string | null;
  message: string;
  /** Set when the host cancelled the open (an explicit close interrupted it). */
  cancelled?: boolean;
}

/** Progress of the native open reported by `library.open_status`. */
interface LibraryOpenStatus {
  path: string;
  stage: 'opening' | 'migrating' | 'loading' | 'starting';
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

function openStageHeadline(stage: LibraryOpenStatus['stage'] | undefined): string {
  switch (stage) {
    case 'migrating':
      return t("Updating your library database");
    case 'loading':
      return t("Loading your library");
    case 'starting':
      return t("Starting library services");
    default:
      return t("Opening your library");
  }
}

export function LibraryGate({ children }: { children: ReactNode }) {
  const [library, setLibrary] = useState<LibraryState>({ kind: 'loading' });
  const [openStatus, setOpenStatus] = useState<LibraryOpenStatus | null>(null);
  const managerRequested = useRef(false);

  // A stage belongs to one attempt at opening one library. Every transition
  // starts without one: a library opening again (switched back to, retried
  // after a failure) must not show where its previous open got to, and
  // nothing is in flight outside 'loading'.
  const transition = useCallback((next: LibraryState) => {
    setOpenStatus(null);
    setLibrary(next);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    void listen<{ path: string }>('library-switched', ({ payload }) => {
      if (!cancelled) {
        setRecentFoldersLibrary(payload.path);
        transition({ kind: 'open', path: payload.path });
      }
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisteners.push(dispose);
    });
    void listen<{ path: string }>('library-switching', ({ payload }) => {
      if (!cancelled) transition({ kind: 'loading', path: payload.path });
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisteners.push(dispose);
    });
    void listen<LibraryOpenFailure>('library-open-failed', () => {
      if (!cancelled) {
        setRecentFoldersLibrary(null);
        transition({ kind: 'closed' });
      }
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisteners.push(dispose);
    });

    void getLibraryConfig()
      .then((config) => {
        if (cancelled) return;
        setRecentFoldersLibrary(config.openingPath ?? config.currentPath);
        transition(config.openingPath
          ? { kind: 'loading', path: config.openingPath }
          : config.currentPath
            ? { kind: 'open', path: config.currentPath }
            : { kind: 'closed' });
      })
      .catch((error) => {
        if (cancelled) return;
        const message = error instanceof Error ? error.message : String(error);
        console.error('[library] failed to read the active library configuration', error);
        setRecentFoldersLibrary(null);
        transition({ kind: 'closed' });
        void invoke('library.initial_read_failed', { message }).catch((deactivationError) => {
          console.error('[library] failed to leave after configuration could not be read', deactivationError);
        });
      });

    return () => {
      cancelled = true;
      unlisteners.forEach((dispose) => dispose());
    };
  }, [transition]);

  const openingPath = library.kind === 'loading' ? library.path ?? null : null;
  // A stage belongs to the library it was polled for: derived at render, a
  // change of the opening path never shows the superseded open's stage, not
  // even for the frame before the poll effect restarts.
  const openStage = openStatus?.path === openingPath ? openStatus.stage : undefined;

  useEffect(() => {
    if (!openingPath) return;
    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const status = await invoke<LibraryOpenStatus | null>('library.open_status');
        // The host reports null once nothing is in flight. Keep the last stage
        // so the headline does not fall back to the generic message. A status
        // for another path is the superseded open still winding down.
        if (!cancelled && status && status.path === openingPath) setOpenStatus(status);
      } catch {
        // The host may still be starting; the next poll retries.
      }
      if (!cancelled) timeout = setTimeout(poll, 100);
    };
    void poll();
    return () => {
      cancelled = true;
      if (timeout) clearTimeout(timeout);
    };
  }, [openingPath]);

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
          <h1>{t("Open a library to start")}</h1>
          <p>{t("Create a new Picto library or open one already on this device.")}</p>
          <button
            className={styles.action}
            type="button"
            onClick={() => void openLibraryManager()}
          >
            {t("Choose Library…")}</button>
        </main>
      ) : library.path ? (
        <main className={styles.content} aria-live="polite">
          <span className={styles.spinner} aria-hidden="true" />
          <h1>{openStageHeadline(openStage)}</h1>
          <div className={styles.progress}>
            <ProgressBar indeterminate height={3} />
          </div>
        </main>
      ) : null}
    </div>
  );
}
