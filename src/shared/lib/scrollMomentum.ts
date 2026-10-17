/**
 * Swallow the tail of a scroll gesture that started on a view now replaced.
 *
 * After a trackpad flick the system keeps sending wheel events (momentum) for
 * a second or more, aimed at whatever is under the pointer. When the view
 * changes underneath (the next item after an Inbox review), that momentum
 * would scroll the new view. The DOM does not mark momentum events, so this
 * treats wheel input that keeps arriving without a pause, and does not grow
 * stronger, as the old gesture: it is cancelled until a short pause, a
 * clearly stronger push (a new flick), or a time limit.
 */

/** No wheel input for this long ends the old gesture. */
const QUIET_MS = 90;
/** Never swallow input for longer than this. */
const LIMIT_MS = 1_200;
/** A push this much stronger than the last event is a new gesture. */
const NEW_GESTURE_RATIO = 1.6;

let release: (() => void) | null = null;

export function suppressScrollMomentum(): void {
  release?.();
  let lastMagnitude = Number.POSITIVE_INFINITY;
  let quiet: ReturnType<typeof setTimeout> | undefined;
  const onWheel = (event: WheelEvent) => {
    const magnitude = Math.abs(event.deltaX) + Math.abs(event.deltaY);
    if (magnitude > lastMagnitude * NEW_GESTURE_RATIO) {
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
  // Without any wheel input, there is no gesture to swallow.
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
