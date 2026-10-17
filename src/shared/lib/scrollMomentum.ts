/**
 * Swallow the tail of a scroll gesture that started on a view now replaced.
 *
 * After a trackpad flick the system keeps sending wheel events (momentum) for
 * a second or more, aimed at whatever is under the pointer. When the view
 * changes underneath (the next item after an Inbox review), that momentum
 * would scroll the new view. The DOM does not mark momentum events, so this
 * treats wheel input that keeps arriving without a pause, in the same
 * direction and no stronger, as the old gesture: it is cancelled until a
 * short pause, a clearly stronger push or a reversal (a new gesture), or a
 * time limit.
 */

/** No wheel input for this long ends the old gesture. */
const QUIET_MS = 90;
/** Never swallow input for longer than this. */
const LIMIT_MS = 1_200;
/** A push this much stronger than the last event is a new gesture. */
const NEW_GESTURE_RATIO = 1.6;

let release: (() => void) | null = null;

// The wheel input seen before the switch: whether a gesture is under way,
// and how strong it was, so the first event after the switch is judged
// against the old gesture rather than assumed to be part of it.
let lastWheelAt = Number.NEGATIVE_INFINITY;
let lastWheelMagnitude = 0;
let lastWheelDirection = 0;

/** Which way the wheel moves along its main axis: -1, 0 or 1. */
function direction(event: WheelEvent): number {
  return Math.sign(Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX);
}

if (typeof window !== 'undefined') {
  window.addEventListener('wheel', (event) => {
    lastWheelAt = Date.now();
    lastWheelMagnitude = Math.abs(event.deltaX) + Math.abs(event.deltaY);
    lastWheelDirection = direction(event);
  }, { capture: true, passive: true });
}

export function suppressScrollMomentum(): void {
  release?.();
  // Without recent wheel input, there is no gesture to swallow.
  if (Date.now() - lastWheelAt > QUIET_MS) return;
  let lastMagnitude = lastWheelMagnitude;
  const gestureDirection = lastWheelDirection;
  let quiet: ReturnType<typeof setTimeout> | undefined;
  const onWheel = (event: WheelEvent) => {
    const magnitude = Math.abs(event.deltaX) + Math.abs(event.deltaY);
    const reversed = gestureDirection !== 0 && direction(event) === -gestureDirection;
    if (reversed || magnitude > lastMagnitude * NEW_GESTURE_RATIO) {
      stop();
      return;
    }
    lastMagnitude = magnitude;
    event.preventDefault();
    event.stopPropagation();
    clearTimeout(quiet);
    quiet = setTimeout(stop, QUIET_MS);
  };
  const limit = setTimeout(() => stop(), LIMIT_MS);
  quiet = setTimeout(stop, QUIET_MS);
  function stop() {
    clearTimeout(quiet);
    clearTimeout(limit);
    window.removeEventListener('wheel', onWheel, { capture: true });
    if (release === stop) release = null;
  }
  window.addEventListener('wheel', onWheel, { capture: true, passive: false });
  release = stop;
}
