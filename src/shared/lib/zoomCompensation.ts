/**
 * Zoom compensation — handles browser zoom (Cmd+/-, Ctrl+/-, or app zoom setting).
 *
 * RULE: In Chromium (Electron), `position: fixed` left/top values are in CSS pixels,
 * but `clientX/clientY` and `getBoundingClientRect()` return visual (zoomed) pixels.
 * `window.innerWidth/innerHeight` and `offsetWidth/offsetHeight` return CSS pixels.
 * This applies to Mac, Windows, and Linux equally.
 *
 * Use these helpers whenever:
 * 1. Converting mouse event coordinates to fixed positioning values
 * 2. Converting getBoundingClientRect() values to fixed positioning values
 * 3. Computing viewport boundaries for clamping fixed-position elements
 *
 * DO NOT use `getBoundingClientRect()` directly for `position: fixed` left/top.
 * DO NOT use `window.innerWidth/innerHeight` directly for clamping without dividing by zoom.
 */

/** Measure the current browser zoom factor (visual pixels / CSS pixels).
 *  Returns 1.0 at 100% zoom. */
export function getZoomFactor(): number {
  const probe = document.createElement('div');
  probe.style.cssText = 'position:fixed;left:0;top:0;width:100px;height:0;pointer-events:none;visibility:hidden;';
  document.body.appendChild(probe);
  const zoom = probe.getBoundingClientRect().width / 100;
  probe.remove();
  return zoom || 1;
}

/** Convert visual (zoomed) coordinates to CSS pixels for position:fixed.
 *  Use for clientX/clientY → fixed left/top. */
export function visualToCSS(visualX: number, visualY: number, zoom?: number): { x: number; y: number } {
  const z = zoom ?? getZoomFactor();
  return { x: visualX / z, y: visualY / z };
}

/** Convert a DOMRect (from getBoundingClientRect) to CSS pixel values.
 *  Use when you need rect positions for fixed positioning or clamping. */
export function rectToCSS(rect: DOMRect, zoom?: number): { left: number; top: number; right: number; bottom: number; width: number; height: number } {
  const z = zoom ?? getZoomFactor();
  return {
    left: rect.left / z,
    top: rect.top / z,
    right: rect.right / z,
    bottom: rect.bottom / z,
    width: rect.width / z,
    height: rect.height / z,
  };
}

/** Get the viewport size in CSS pixels, adjusted for zoom.
 *  Use for clamping fixed-position elements to the window. */
export function getViewportCSS(zoom?: number): { width: number; height: number } {
  const z = zoom ?? getZoomFactor();
  return {
    width: window.innerWidth / z,
    height: window.innerHeight / z,
  };
}
