export interface RuffleMovieMetadata {
  width?: number;
  height?: number;
}

export interface RufflePlayerV1 {
  load(options: RuffleLoadOptions): Promise<void>;
  readonly readyState: number;
  readonly metadata: RuffleMovieMetadata | null;
  readonly suspended: boolean;
  volume: number;
  resume(): void;
  suspend(): void;
}

export interface RufflePlayerElement extends HTMLElement {
  ruffle(version: 1): RufflePlayerV1;
}

interface RuffleSource {
  createPlayer(): RufflePlayerElement;
}

interface RuffleGlobal {
  newest?: () => RuffleSource;
  config?: { publicPath?: string; [key: string]: unknown };
}

declare global {
  interface Window {
    RufflePlayer?: RuffleGlobal;
  }
}

export interface RuffleLoadOptions {
  url: string;
  autoplay: 'on' | 'off';
  unmuteOverlay: 'hidden';
  allowScriptAccess: false;
  allowNetworking: 'internal';
  openUrlMode: 'confirm';
  contextMenu: 'off';
  showSwfDownload: false;
  allowFullscreen: false;
  menu: false;
  preloader: false;
  splashScreen: false;
}

let runtimePromise: Promise<RuffleGlobal> | null = null;

export function loadRuffleRuntime(): Promise<RuffleGlobal> {
  if (window.RufflePlayer?.newest) return Promise.resolve(window.RufflePlayer);
  if (runtimePromise) return runtimePromise;

  runtimePromise = new Promise((resolve, reject) => {
    const existing = document.querySelector<HTMLScriptElement>('script[data-picto-ruffle]');
    const script = existing ?? document.createElement('script');
    const finish = () => {
      if (window.RufflePlayer?.newest) resolve(window.RufflePlayer);
      else reject(new Error('Ruffle loaded without exposing its versioned player API.'));
    };
    script.addEventListener('load', finish, { once: true });
    script.addEventListener('error', () => reject(new Error('Could not load the Flash runtime.')), { once: true });
    if (!existing) {
      const publicPath = new URL('vendor/ruffle/', document.baseURI).href;
      window.RufflePlayer ??= {};
      window.RufflePlayer.config = { ...window.RufflePlayer.config, publicPath };
      script.dataset.pictoRuffle = '';
      script.src = `${publicPath}ruffle.js`;
      document.head.appendChild(script);
    }
  });
  return runtimePromise;
}

export async function createRufflePlayer(): Promise<RufflePlayerElement> {
  const runtime = await loadRuffleRuntime();
  const source = runtime.newest?.();
  if (!source) throw new Error('The Flash runtime did not expose a player source.');
  const player = source.createPlayer();
  player.ruffle(1); // Fail immediately when an upgrade removes Picto's supported API version.
  return player;
}

export function loadRuffleMovie(player: RufflePlayerElement, url: string, autoplay: 'on' | 'off') {
  return player.ruffle(1).load({
    url,
    autoplay,
    unmuteOverlay: 'hidden',
    allowScriptAccess: false,
    allowNetworking: 'internal',
    openUrlMode: 'confirm',
    contextMenu: 'off',
    showSwfDownload: false,
    allowFullscreen: false,
    menu: false,
    preloader: false,
    splashScreen: false,
  });
}

/**
 * Public config suppresses Ruffle's menu and loading splash, but it still paints a play overlay
 * while suspended. Keep this sole internal-DOM dependency at the adapter edge.
 */
export function applyPictoRuffleChrome(player: RufflePlayerElement) {
  const shadow = player.shadowRoot;
  if (!shadow || shadow.querySelector('[data-picto-ruffle-chrome]')) return;
  const style = document.createElement('style');
  style.dataset.pictoRuffleChrome = '';
  style.textContent = '#play-button, #splash-screen, #unmute-overlay { display: none !important; }';
  shadow.appendChild(style);
}
