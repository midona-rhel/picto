/** Video player constants — single source of truth for all magic numbers. */

export const PLAYBACK_RATES = [0.25, 0.5, 0.75, 1, 2, 3] as const;

export const DEFAULT_VOLUME = 0.9;
export const CONTROLS_HIDE_DELAY = 2500; // ms before auto-hiding controls
export const VOLUME_STEP = 0.05; // per arrow key press
export const VOLUME_SCROLL_STEP = 0.05; // per scroll tick (alt+scroll)
export const SEEK_SCROLL_STEP = 5; // seconds per scroll tick
export const VOLUME_HUD_DURATION = 800; // ms before HUD fades
export const SKIP_STEP = 5; // seconds per skip button press
export const DEFAULT_FPS = 30; // fallback when detection isn't possible
