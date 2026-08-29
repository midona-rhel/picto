import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import styles from './ColorFilterEditor.module.css';

const PRESETS = [
  '#111111', '#FFFFFF', '#9E9E9E', '#A48057', '#FC85B3', '#FF2727',
  '#FFA34B', '#FFD534', '#47C595', '#51C4C4', '#2B76E7', '#6D50ED',
] as const;

interface HsvColor { hue: number; saturation: number; value: number }

function hexToHsv(hex: string): HsvColor {
  const normalized = /^#[0-9a-f]{6}$/i.test(hex) ? hex.slice(1) : '808080';
  const red = parseInt(normalized.slice(0, 2), 16) / 255;
  const green = parseInt(normalized.slice(2, 4), 16) / 255;
  const blue = parseInt(normalized.slice(4, 6), 16) / 255;
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const delta = max - min;
  const hue = delta === 0 ? 0
    : max === red ? 60 * (((green - blue) / delta) % 6)
    : max === green ? 60 * ((blue - red) / delta + 2)
    : 60 * ((red - green) / delta + 4);
  return {
    hue: hue < 0 ? hue + 360 : hue,
    saturation: max === 0 ? 0 : delta / max,
    value: max,
  };
}

function hsvToHex({ hue, saturation, value }: HsvColor): string {
  const chroma = value * saturation;
  const segment = hue / 60;
  const secondary = chroma * (1 - Math.abs((segment % 2) - 1));
  const [red, green, blue] = segment < 1 ? [chroma, secondary, 0]
    : segment < 2 ? [secondary, chroma, 0]
    : segment < 3 ? [0, chroma, secondary]
    : segment < 4 ? [0, secondary, chroma]
    : segment < 5 ? [secondary, 0, chroma]
    : [chroma, 0, secondary];
  const match = value - chroma;
  const byte = (component: number) => Math.round((component + match) * 255).toString(16).padStart(2, '0');
  return `#${byte(red)}${byte(green)}${byte(blue)}`.toUpperCase();
}

function useDeferredCommit(onCommit: (value: string | null) => void) {
  const callback = useRef(onCommit);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  callback.current = onCommit;
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);
  return useCallback((value: string) => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => callback.current(value), 80);
  }, []);
}

export function ColorFilterEditor({ value, onCommit, allowClear = true }: {
  value: string | null;
  onCommit: (value: string | null) => void;
  allowClear?: boolean;
}) {
  const initial = value?.toUpperCase() ?? '#808080';
  const [hex, setHex] = useState(initial);
  const [hsv, setHsv] = useState(() => hexToHsv(initial));
  const scheduleCommit = useDeferredCommit(onCommit);

  useEffect(() => {
    const next = value?.toUpperCase() ?? '#808080';
    setHex(next);
    setHsv(hexToHsv(next));
  }, [value]);

  const applyHsv = (next: HsvColor) => {
    const nextHex = hsvToHex(next);
    setHsv(next);
    setHex(nextHex);
    scheduleCommit(nextHex);
  };
  const selectSaturationValue = (event: ReactPointerEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    applyHsv({
      hue: hsv.hue,
      saturation: Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)),
      value: 1 - Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height)),
    });
  };

  return (
    <div className={styles.editor}>
      <div
        className={styles.saturation}
        style={{ '--filter-hue': `hsl(${hsv.hue} 100% 50%)` } as CSSProperties}
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId);
          selectSaturationValue(event);
        }}
        onPointerMove={(event) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId)) selectSaturationValue(event);
        }}
        onPointerUp={(event) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
        }}
        aria-label="Color saturation and brightness"
      >
        <span className={styles.cursor} style={{ left: `${hsv.saturation * 100}%`, top: `${(1 - hsv.value) * 100}%` }} />
      </div>
      <input className={styles.hue} type="range" min="0" max="359" value={Math.round(hsv.hue)} aria-label="Color hue" onChange={(event) => applyHsv({ ...hsv, hue: Number(event.target.value) })} />
      <div className={styles.presets}>
        {allowClear && <button type="button" className={`${styles.preset} ${styles.none} ${value == null ? styles.active : ''}`} aria-label="No color" onClick={() => onCommit(null)} />}
        {PRESETS.map((preset) => (
          <button type="button" key={preset} className={`${styles.preset} ${hex === preset ? styles.active : ''}`} style={{ background: preset }} aria-label={preset} onClick={() => onCommit(preset)} />
        ))}
      </div>
      <label className={styles.hexField}>
        <span className={styles.preview} style={{ background: /^#[0-9a-f]{6}$/i.test(hex) ? hex : 'transparent' }} />
        <input aria-label="Hex color" value={hex} maxLength={7} onChange={(event) => {
          const next = event.target.value.toUpperCase();
          setHex(next);
          if (/^#[0-9A-F]{6}$/.test(next)) {
            setHsv(hexToHsv(next));
            scheduleCommit(next);
          }
        }} />
      </label>
    </div>
  );
}
