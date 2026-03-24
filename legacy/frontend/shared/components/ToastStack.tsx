import { useEffect, useRef, useState } from 'react';
import { useSyncExternalStore } from 'react';
import styles from './ToastStack.module.css';

// ── Toast store (module-level, framework-agnostic) ──────────────────────────

type ToastType = 'success' | 'error' | 'warning' | 'info';

interface Toast {
  id: number;
  message: string;
  type: ToastType;
  autoClose: number;
}

let _toasts: Toast[] = [];
let _nextId = 1;
const _listeners = new Set<() => void>();
function _notify() { _listeners.forEach(fn => fn()); }

const DEFAULT_AUTO_CLOSE: Record<ToastType, number> = {
  success: 3000,
  error: 5000,
  warning: 4000,
  info: 3000,
};

export function addToast(message: string, type: ToastType, autoClose?: number): number {
  const id = _nextId++;
  _toasts = [..._toasts, { id, message, type, autoClose: autoClose ?? DEFAULT_AUTO_CLOSE[type] }];
  _notify();
  return id;
}

export function removeToast(id: number): void {
  _toasts = _toasts.filter(t => t.id !== id);
  _notify();
}

function getToasts(): Toast[] { return _toasts; }

// ── Individual toast item ───────────────────────────────────────────────────

const DOT_CLASS: Record<ToastType, string> = {
  success: styles.dotSuccess,
  error: styles.dotError,
  warning: styles.dotWarning,
  info: styles.dotInfo,
};

function ToastItem({ toast }: { toast: Toast }) {
  const [exiting, setExiting] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    timerRef.current = setTimeout(() => {
      setExiting(true);
      setTimeout(() => removeToast(toast.id), 150);
    }, toast.autoClose);
    return () => { if (timerRef.current) clearTimeout(timerRef.current); };
  }, [toast.id, toast.autoClose]);

  return (
    <div className={`${styles.toast}${exiting ? ` ${styles.exiting}` : ''}`}>
      <span className={`${styles.dot} ${DOT_CLASS[toast.type]}`} />
      <span className={styles.message}>{toast.message}</span>
    </div>
  );
}

// ── Stack component ─────────────────────────────────────────────────────────

export function ToastStack() {
  const toasts = useSyncExternalStore(
    (cb) => { _listeners.add(cb); return () => { _listeners.delete(cb); }; },
    getToasts,
  );

  if (toasts.length === 0) return null;

  return (
    <div className={styles.container}>
      {toasts.map(t => <ToastItem key={t.id} toast={t} />)}
    </div>
  );
}
