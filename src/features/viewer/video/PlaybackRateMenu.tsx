import { useEffect, useRef, useState } from 'react';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import { PLAYBACK_RATES } from './videoConstants';
import styles from './VideoPlayer.module.css';
import { t } from '../../../i18n';

interface Props { rate: number; onRateChange: (r: number) => void; }

export function PlaybackRateMenu({ rate, onRateChange }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    document.addEventListener('mousedown', handler, true);
    return () => document.removeEventListener('mousedown', handler, true);
  }, [open]);

  return (
    <div ref={ref} className={styles.rateMenuContainer}>
      <KbdTooltip label={t("Playback speed")}>
        <button className={`${styles.icBtn} ${styles.rateButton}`} onClick={(e) => { e.stopPropagation(); setOpen(p => !p); }}>
          {rate === 1 ? t("1x") : t("{value0}x", { value0: rate })}
        </button>
      </KbdTooltip>
      {open && (
        <div className={styles.rateMenu}>
          {PLAYBACK_RATES.map(r => (
            <button key={r} className={`${styles.rateMenuItem} ${r === rate ? styles.rateMenuItemActive : ''}`}
              onClick={(e) => { e.stopPropagation(); onRateChange(r); setOpen(false); }}>
              {r}{t("x")}</button>
          ))}
        </div>
      )}
    </div>
  );
}
