import { createRufflePlayer, loadRuffleMovie } from '../shared/flash/ruffleRuntime';

interface FlashThumbnailState {
  ready?: boolean;
  error?: string;
  width?: number;
  height?: number;
}

declare global {
  interface Window {
    __pictoFlashThumbnail?: FlashThumbnailState;
  }
}

async function render() {
  const src = new URLSearchParams(location.search).get('src');
  if (!src) throw new Error('Missing Flash source.');

  const player = await createRufflePlayer();
  const runtime = player.ruffle(1);
  runtime.volume = 0;
  document.querySelector('#player')?.appendChild(player);

  let markedReady = false;
  const markReady = () => {
    if (markedReady) return;
    markedReady = true;
    const metadata = runtime.metadata;
    window.__pictoFlashThumbnail = {
      ready: true,
      width: metadata?.width,
      height: metadata?.height,
    };
  };
  player.addEventListener('loadeddata', markReady, { once: true });
  await loadRuffleMovie(player, src, 'on');
  if (runtime.readyState === 2) markReady();
}

void render().catch((error: unknown) => {
  window.__pictoFlashThumbnail = {
    error: error instanceof Error ? error.message : 'Could not render this Flash file.',
  };
});

export {};
