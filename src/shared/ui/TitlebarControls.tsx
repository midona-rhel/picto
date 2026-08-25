import {
  forwardRef,
  type ButtonHTMLAttributes,
  type ChangeEvent,
  type InputHTMLAttributes,
  type ReactNode,
} from 'react';
import { KbdTooltip } from './KbdTooltip';
import { ToolbarMinusIcon, ToolbarPlusIcon } from './icons/toolbar-icons';
import styles from './TitlebarControls.module.css';

interface TitlebarControlsProps {
  label?: string;
  left?: ReactNode;
  center?: ReactNode;
  right?: ReactNode;
}

export function TitlebarControls({ label, left, center, right }: TitlebarControlsProps) {
  return (
    <div className={styles.toolbar} aria-label={label} data-window-drag-region="">
      {left ? <div className={styles.left}>{left}</div> : null}
      <div className={styles.center}>{center}</div>
      {right ? <div className={styles.right}>{right}</div> : null}
    </div>
  );
}

interface TitlebarControlButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
}

export const TitlebarControlButton = forwardRef<HTMLButtonElement, TitlebarControlButtonProps>(
  function TitlebarControlButton({ active = false, className = '', disabled, ...props }, ref) {
    const classes = [
      styles.button,
      active ? styles.buttonActive : '',
      disabled ? styles.buttonDisabled : '',
      className,
    ].filter(Boolean).join(' ');
    return <button ref={ref} className={classes} disabled={disabled} {...props} />;
  },
);

export function TitlebarControlGroup({ children }: { children: ReactNode }) {
  return <div className={styles.group}>{children}</div>;
}

export function TitlebarCounter({ current, total }: { current: number; total: number }) {
  return <span className={styles.counter}>{current} / {total}</span>;
}

export function TitlebarValue({ children }: { children: ReactNode }) {
  return <span className={styles.value}>{children}</span>;
}

interface TitlebarZoomSliderProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  onZoomOut: () => void;
  onZoomIn: () => void;
}

interface TitlebarRangeSliderProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'onChange'> {
  onValueChange: (value: number) => void;
}

export const TitlebarRangeSlider = forwardRef<HTMLInputElement, TitlebarRangeSliderProps>(function TitlebarRangeSlider(
  { onValueChange, className = '', ...props },
  ref,
) {
  const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
    onValueChange(Number(event.target.value));
  };

  return (
    <input
      {...props}
      ref={ref}
      type="range"
      onChange={handleChange}
      className={[styles.zoomSlider, className].filter(Boolean).join(' ')}
    />
  );
});

export function TitlebarZoomSlider({
  value,
  min,
  max,
  step = 1,
  onChange,
  onZoomOut,
  onZoomIn,
}: TitlebarZoomSliderProps) {
  return (
    <div className={styles.zoomControl}>
      <KbdTooltip label="Zoom out" shortcutId="view.zoomOut">
        <TitlebarControlButton onClick={onZoomOut} disabled={value <= min} aria-label="Zoom out">
          <ToolbarMinusIcon />
        </TitlebarControlButton>
      </KbdTooltip>
      <TitlebarRangeSlider
        aria-label="Zoom"
        min={min}
        max={max}
        step={step}
        value={value}
        onValueChange={onChange}
      />
      <KbdTooltip label="Zoom in" shortcutId="view.zoomIn">
        <TitlebarControlButton onClick={onZoomIn} disabled={value >= max} aria-label="Zoom in">
          <ToolbarPlusIcon />
        </TitlebarControlButton>
      </KbdTooltip>
    </div>
  );
}
