import { useEffect, useState } from 'react';
import { IconDownload, IconRefresh } from '@tabler/icons-react';
import { GlassModal, modalStyles } from '../../shared/ui/GlassModal';
import {
  checkForUpdates,
  getUpdateState,
  installUpdate,
  onUpdateState,
  openUpdateRelease,
  type UpdateState,
} from '../../platform/updateApi';
import styles from './UpdateModal.module.css';

function NoteBody({ text }: { text: string }) {
  const lines = text.trim().split(/\r?\n/);
  if (!text.trim()) return <p className={styles.emptyNotes}>No release notes were provided.</p>;
  return <div className={styles.notes}>{lines.map((line, index) => {
    const heading = line.match(/^#{1,3}\s+(.+)/);
    const bullet = line.match(/^[-*]\s+(.+)/);
    if (heading) return <h3 key={index}>{heading[1]}</h3>;
    if (bullet) return <div className={styles.noteItem} key={index}><span>•</span><span>{bullet[1]}</span></div>;
    if (!line.trim()) return <div className={styles.noteGap} key={index} />;
    return <p key={index}>{line.replace(/\*\*/g, '')}</p>;
  })}</div>;
}

export function UpdateModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [state, setState] = useState<UpdateState | null>(null);
  useEffect(() => {
    if (!open) return;
    void getUpdateState().then(setState);
    let dispose: (() => void) | undefined;
    void onUpdateState(setState).then((value) => { dispose = value; });
    return () => dispose?.();
  }, [open]);

  const busy = state?.status === 'checking' || state?.status === 'downloading';
  const action = state?.platform === 'darwin'
    ? () => void openUpdateRelease()
    : () => void installUpdate();
  const actionLabel = state?.platform === 'darwin' ? 'Open Download Page' : 'Restart and Install';

  return <GlassModal
    open={open}
    onClose={onClose}
    title={state?.version ? `Picto ${state.version}` : 'Software Update'}
    size="md"
    footer={<>
      <button className={modalStyles.btn} type="button" disabled={busy} onClick={() => void checkForUpdates().then(setState)}>
        <IconRefresh size={15} /> Check Again
      </button>
      {state?.status === 'downloaded' || (state?.status === 'available' && state.platform === 'darwin') ? (
        <button className={`${modalStyles.btn} ${modalStyles.btnPrimary}`} type="button" onClick={action}>
          <IconDownload size={15} /> {actionLabel}
        </button>
      ) : null}
    </>}
  >
    <div className={styles.summary}>
      <div className={styles.version}>Current version {state?.currentVersion ?? '—'}</div>
      {state?.status === 'checking' ? <p>Checking for the latest version…</p> : null}
      {state?.status === 'current' ? <p>Picto is up to date.</p> : null}
      {state?.status === 'unavailable' ? <p>{state.error}</p> : null}
      {state?.status === 'error' ? <p className={styles.error}>{state.error}</p> : null}
      {state?.status === 'downloading' ? <>
        <p>Downloading Picto {state.version}… {state.progress ? `${Math.round(state.progress.percent)}%` : ''}</p>
        <div className={styles.progress}><span style={{ width: `${state.progress?.percent ?? 2}%` }} /></div>
      </> : null}
      {state?.status === 'downloaded' ? <p>The update is ready. Picto will close before the installer starts.</p> : null}
      {state?.status === 'available' && state.platform === 'darwin' ? <p>A new version is available. Download it from the release page to update this Mac.</p> : null}
    </div>
    {state?.version ? <section className={styles.release}>
      <div className={styles.releaseHeader}>
        <strong>{state.releaseName || `What’s new in ${state.version}`}</strong>
        {state.releaseDate ? <time>{new Date(state.releaseDate).toLocaleDateString()}</time> : null}
      </div>
      <NoteBody text={state.releaseNotes} />
    </section> : null}
  </GlassModal>;
}
