import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { IconCopy, IconDownload, IconTrash, IconX } from '@tabler/icons-react';
import {
  clearDiagnostics,
  useDiagnostics,
  type DiagnosticLevel,
  type DiagnosticSource,
} from './diagnosticsStore';
import { invoke } from '../../platform/ipc';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import {
  buildSupportReport,
  formatDiagnosticDisplayEntry,
  formatDiagnosticEntry,
} from './supportReport';
import styles from './DiagnosticsPanel.module.css';
import { t } from '../../i18n';

const LEVELS: DiagnosticLevel[] = ['ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'];
const SOURCES: Array<'all' | DiagnosticSource> = ['all', 'core', 'main', 'renderer', 'ipc'];

interface WorkerDiagnostic {
  id: string;
  label: string;
  state: 'working' | 'waiting' | 'attention';
  detail: string;
  active: number;
  queued: number;
  attention: number;
}

interface DiagnosticsSnapshot {
  workers: WorkerDiagnostic[];
}

function formatTime(timestamp: string) {
  const date = new Date(timestamp);
  return `${date.toLocaleTimeString(undefined, { hour12: false })}.${String(date.getMilliseconds()).padStart(3, '0')}`;
}

export function DiagnosticsPanel({ onClose }: { onClose: () => void }) {
  const entries = useDiagnostics();
  const [levels, setLevels] = useState(() => new Set<DiagnosticLevel>(LEVELS));
  const [source, setSource] = useState<'all' | DiagnosticSource>('all');
  const [search, setSearch] = useState('');
  const [workers, setWorkers] = useState<WorkerDiagnostic[]>([]);
  const [height, setHeight] = useState<number | null>(null);
  const panelRef = useRef<HTMLElement>(null);
  const resizeRef = useRef<{ pointerId: number; startY: number; startHeight: number } | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const followRef = useRef(true);

  useEffect(() => {
    let active = true;
    const refresh = async () => {
      try {
        const snapshot = await invoke<DiagnosticsSnapshot>('diagnostics.snapshot');
        if (active) setWorkers(snapshot.workers ?? []);
      } catch {
        // The log stream already reports transport failures.
      }
    };
    void refresh();
    const interval = window.setInterval(refresh, 750);
    return () => { active = false; window.clearInterval(interval); };
  }, []);

  useShortcutScope((event) => {
    if (event.key !== 'Escape') return false;
    onClose();
    return true;
  }, { priority: 100, allowInEditable: true });

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase();
    return entries.filter((entry) => levels.has(entry.level)
      && (source === 'all' || entry.source === source)
      && (!query || `${entry.target} ${entry.message}`.toLowerCase().includes(query)));
  }, [entries, levels, search, source]);

  useEffect(() => {
    if (followRef.current && listRef.current) listRef.current.scrollTop = listRef.current.scrollHeight;
  }, [filtered.length]);

  const toggleLevel = useCallback((level: DiagnosticLevel) => {
    setLevels((current) => {
      const next = new Set(current);
      if (next.has(level)) next.delete(level); else next.add(level);
      return next;
    });
  }, []);

  const copy = () => navigator.clipboard.writeText(filtered.map((entry) =>
    formatDiagnosticEntry(entry),
  ).join('\n'));

  const saveSupportReport = async () => {
    await (window as any).picto.diagnostics.save(buildSupportReport(entries, workers, {
      userAgent: navigator.userAgent,
      language: navigator.language,
    }));
  };

  const resizeTo = useCallback((nextHeight: number) => {
    setHeight(Math.max(260, Math.min(window.innerHeight - 80, nextHeight)));
  }, []);

  return (
    <section
      ref={panelRef}
      className={styles.panel}
      aria-label={t("Diagnostics")}
      data-picto-text-shortcuts
      tabIndex={-1}
      style={height == null ? undefined : { height }}
    >
      <div
        className={styles.resizeHandle}
        role="separator"
        aria-label={t("Resize diagnostics")}
        aria-orientation="horizontal"
        tabIndex={0}
        onPointerDown={(event) => {
          resizeRef.current = {
            pointerId: event.pointerId,
            startY: event.clientY,
            startHeight: panelRef.current?.getBoundingClientRect().height ?? 260,
          };
          event.currentTarget.setPointerCapture(event.pointerId);
          event.preventDefault();
        }}
        onPointerMove={(event) => {
          const resize = resizeRef.current;
          if (!resize || resize.pointerId !== event.pointerId) return;
          resizeTo(resize.startHeight + resize.startY - event.clientY);
        }}
        onPointerUp={(event) => {
          if (resizeRef.current?.pointerId !== event.pointerId) return;
          resizeRef.current = null;
          event.currentTarget.releasePointerCapture(event.pointerId);
        }}
        onPointerCancel={() => { resizeRef.current = null; }}
        onKeyDown={(event) => {
          if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
          const current = panelRef.current?.getBoundingClientRect().height ?? 260;
          resizeTo(current + (event.key === 'ArrowUp' ? 24 : -24));
          event.preventDefault();
        }}
      />
      <header className={styles.header}>
        <strong className={styles.title}>{t("Logs")}</strong>
        <div className={styles.filters} aria-label={t("Log levels")}>
          {LEVELS.map((level) => (
            <button key={level} className={levels.has(level) ? styles.filterActive : styles.filter} onClick={() => toggleLevel(level)}>{level}</button>
          ))}
        </div>
        <div className={styles.filters} aria-label={t("Log sources")}>
          {SOURCES.map((item) => (
            <button key={item} className={source === item ? styles.filterActive : styles.filter} onClick={() => setSource(item)}>{item}</button>
          ))}
        </div>
        <input className={styles.search} value={search} onChange={(event) => setSearch(event.target.value)} placeholder={t("Filter logs")} />
        <KbdTooltip label={t("Copy visible logs")}><button className={styles.iconButton} onClick={() => { void copy(); }} aria-label={t("Copy visible logs")}><IconCopy size={14} /></button></KbdTooltip>
        <KbdTooltip label={t("Save support report")}><button className={styles.iconButton} onClick={() => { void saveSupportReport(); }} aria-label={t("Save support report")}><IconDownload size={14} /></button></KbdTooltip>
        <KbdTooltip label={t("Clear logs")}><button className={styles.iconButton} onClick={clearDiagnostics} aria-label={t("Clear logs")}><IconTrash size={14} /></button></KbdTooltip>
        <KbdTooltip label={t("Close diagnostics")}><button className={styles.iconButton} onClick={onClose} aria-label={t("Close diagnostics")}><IconX size={15} /></button></KbdTooltip>
      </header>
      <div className={styles.content}>
        <div
          ref={listRef}
          className={styles.logList}
          onScroll={(event) => {
            const element = event.currentTarget;
            followRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 32;
          }}
        >
          {filtered.map((entry) => (
            <div className={styles.logRow} key={entry.id} data-level={entry.level}>
              {formatDiagnosticDisplayEntry(entry, formatTime(entry.timestamp))}
            </div>
          ))}
        </div>
        <aside className={styles.workers} aria-label={t("Workers")}>
          <div className={styles.workersTitle}>{t("Workers")}</div>
          {workers.map((worker) => (
            <div className={styles.worker} key={worker.id}>
              <span className={styles.workerDot} data-state={worker.state} />
              <div className={styles.workerText}>
                <div className={styles.workerName}>{worker.label}</div>
                <div className={styles.workerDetail}>{worker.detail}</div>
              </div>
              {worker.active + worker.queued > 0 ? (
                <span className={styles.workerCount}>
                  {worker.active > 0 ? t("{value0} running", { value0: worker.active }) : t("{value0} queued", { value0: worker.queued })}
                </span>
              ) : null}
            </div>
          ))}
        </aside>
      </div>
    </section>
  );
}
