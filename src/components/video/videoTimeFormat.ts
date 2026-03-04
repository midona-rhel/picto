/** Video time formatting utilities. */

/** Format seconds as "H:MM:SS" or "M:SS" or "0:SS". */
export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00';
  const total = Math.floor(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) {
    return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
  }
  return `${m}:${s.toString().padStart(2, '0')}`;
}

/** Format seconds with frame fraction, e.g. "1:23:45.12". */
export function formatFrameTime(seconds: number, fps: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00.00';
  const whole = Math.floor(seconds);
  const frac = seconds - whole;
  const frameNum = Math.floor(frac * fps);
  const base = formatTime(whole);
  return `${base}.${frameNum.toString().padStart(2, '0')}`;
}
