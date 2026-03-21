import { useCallback, useEffect, useRef, useState } from 'react';
import { useLogStore, type LogEntry } from '../../../state/logStore';
import st from './LogPanel.module.css';

const LEVELS = ['ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'] as const;

function formatTime(timestamp: string): string {
  try {
    const d = new Date(timestamp);
    return d.toLocaleTimeString(undefined, { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })
      + '.' + String(d.getMilliseconds()).padStart(3, '0');
  } catch {
    return timestamp.slice(11, 23);
  }
}

function levelClass(level: string): string {
  switch (level) {
    case 'ERROR': return st.levelError;
    case 'WARN': return st.levelWarn;
    case 'INFO': return st.levelInfo;
    case 'DEBUG': return st.levelDebug;
    case 'TRACE': return st.levelTrace;
    default: return st.levelInfo;
  }
}

export function LogPanel({ onClose }: { onClose: () => void }) {
  const entries = useLogStore((s) => s.entries);
  const clear = useLogStore((s) => s.clear);

  const [enabledLevels, setEnabledLevels] = useState<Set<string>>(() => new Set(['ERROR', 'WARN', 'INFO']));
  const [search, setSearch] = useState('');
  const listRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  const toggleLevel = useCallback((level: string) => {
    setEnabledLevels((prev) => {
      const next = new Set(prev);
      if (next.has(level)) next.delete(level);
      else next.add(level);
      return next;
    });
  }, []);

  const filtered = entries.filter((e) => {
    if (!enabledLevels.has(e.level)) return false;
    if (search && !e.message.toLowerCase().includes(search.toLowerCase()) && !e.target.toLowerCase().includes(search.toLowerCase())) return false;
    return true;
  });

  // Auto-scroll to bottom
  useEffect(() => {
    if (autoScrollRef.current && listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [filtered.length]);

  const handleScroll = useCallback(() => {
    const el = listRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    autoScrollRef.current = atBottom;
  }, []);

  const handleCopy = useCallback(() => {
    const text = filtered.map((e) => `${e.timestamp} ${e.level} ${e.target} ${e.message}`).join('\n');
    void navigator.clipboard.writeText(text);
  }, [filtered]);

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [onClose]);

  return (
    <div className={st.overlay}>
      <div className={st.header}>
        <span className={st.title}>Logs</span>
        <div className={st.filterPills}>
          {LEVELS.map((level) => (
            <button
              key={level}
              className={`${enabledLevels.has(level) ? st.pillActive : st.pill} ${level === 'ERROR' ? st.pillError : level === 'WARN' ? st.pillWarn : ''}`}
              onClick={() => toggleLevel(level)}
            >
              {level}
            </button>
          ))}
        </div>
        <input
          className={st.searchInput}
          placeholder="Filter..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <button className={st.headerBtn} onClick={handleCopy}>Copy</button>
        <button className={st.headerBtn} onClick={clear}>Clear</button>
        <button className={st.headerBtn} onClick={onClose}>Close</button>
      </div>
      <div className={st.logList} ref={listRef} onScroll={handleScroll}>
        {filtered.map((entry) => (
          <LogRow key={entry.id} entry={entry} />
        ))}
      </div>
    </div>
  );
}

function LogRow({ entry }: { entry: LogEntry }) {
  return (
    <div className={st.logRow}>
      <span className={st.logTime}>{formatTime(entry.timestamp)}</span>
      <span className={`${st.logLevel} ${levelClass(entry.level)}`}>{entry.level}</span>
      <span className={st.logTarget}>{entry.target}</span>
      <span className={st.logMessage}>{entry.message}</span>
    </div>
  );
}
