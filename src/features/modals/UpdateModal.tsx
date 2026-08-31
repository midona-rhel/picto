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
import { t } from '../../i18n';

type NoteBlock = { kind: 'heading' | 'paragraph' | 'bullet'; text: string; level?: number };

export function parseReleaseNotes(text: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  let current: NoteBlock | null = null;
  const flush = () => {
    if (current?.text) blocks.push(current);
    current = null;
  };

  for (const rawLine of text.trim().split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) {
      flush();
      continue;
    }
    const heading = line.match(/^(#{1,3})\s+(.+)/);
    if (heading) {
      flush();
      blocks.push({ kind: 'heading', level: heading[1].length, text: heading[2] });
      continue;
    }
    const bullet = line.match(/^[-*]\s+(.+)/);
    if (bullet) {
      flush();
      current = { kind: 'bullet', text: bullet[1] };
      continue;
    }
    if (!current) current = { kind: 'paragraph', text: line };
    else current.text += ` ${line}`;
  }
  flush();
  return blocks;
}

function NoteBody({ text }: { text: string }) {
  if (!text.trim()) return <p className={styles.emptyNotes}>{t("No release notes were provided.")}</p>;
  const blocks = parseReleaseNotes(text);
  if (blocks[0]?.kind === 'heading' && blocks[0].level === 1) blocks.shift();
  return <div className={styles.notes}>{blocks.map((block, index) => {
    const content = block.text.replace(/\*\*/g, '');
    if (block.kind === 'heading') return <h3 key={index}>{content}</h3>;
    if (block.kind === 'bullet') return <div className={styles.noteItem} key={index}><span aria-hidden="true">•</span><span>{content}</span></div>;
    return <p key={index}>{content}</p>;
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
    title={state?.version ? t("Picto {value0}", { value0: state.version }) : t("Software Update")}
    size="md"
    footer={<>
      <button className={modalStyles.btn} type="button" disabled={busy} onClick={() => void checkForUpdates().then(setState)}>
        <IconRefresh size={15} /> {t("Check Again")}</button>
      {state?.status === 'downloaded' || (state?.status === 'available' && state.platform === 'darwin') ? (
        <button className={`${modalStyles.btn} ${modalStyles.btnPrimary}`} type="button" onClick={action}>
          <IconDownload size={15} /> {actionLabel}
        </button>
      ) : null}
    </>}
  >
    <div className={styles.summary}>
      <div className={styles.version}>{t("Current version ")}{state?.currentVersion ?? '—'}</div>
      {state?.status === 'checking' ? <p>{t("Checking for the latest version…")}</p> : null}
      {state?.status === 'current' ? <p>{t("Picto is up to date.")}</p> : null}
      {state?.status === 'unavailable' ? <p>{state.error}</p> : null}
      {state?.status === 'error' ? <p className={styles.error}>{state.error}</p> : null}
      {state?.status === 'downloading' ? <>
        <p>{t("Downloading Picto ")}{state.version}… {state.progress ? `${Math.round(state.progress.percent)}%` : ''}</p>
        <div className={styles.progress}><span style={{ width: `${state.progress?.percent ?? 2}%` }} /></div>
      </> : null}
      {state?.status === 'downloaded' ? <p>{t("The update is ready. Picto will close before the installer starts.")}</p> : null}
      {state?.status === 'available' && state.platform === 'darwin' ? <p>{t("A new version is available. Download it from the release page to update this Mac.")}</p> : null}
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
