import { afterEach, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ create: vi.fn(), load: vi.fn() }));
vi.mock('../shared/flash/ruffleRuntime', () => ({
  createRufflePlayer: mocks.create,
  loadRuffleMovie: mocks.load,
}));

afterEach(() => {
  document.body.replaceChildren();
  delete window.__pictoFlashThumbnail;
  vi.resetModules();
  vi.resetAllMocks();
});

it('loads paused and mutes the initialized runtime before playing thumbnail frames', async () => {
  history.replaceState({}, '', '/?src=media://localhost/file/test.swf');
  document.body.innerHTML = '<div id="player"></div>';
  let initialized = false;
  let volume = 1;
  const resume = vi.fn(() => {
    expect(initialized).toBe(true);
    expect(volume).toBe(0);
  });
  const runtime = {
    readyState: 2,
    metadata: { width: 400, height: 200 },
    set volume(value: number) { if (initialized) volume = value; },
    resume,
  };
  const player = Object.assign(document.createElement('div'), { ruffle: () => runtime });
  mocks.create.mockResolvedValue(player);
  mocks.load.mockImplementation(async (_player, _src, autoplay) => {
    expect(autoplay).toBe('off');
    initialized = true;
    player.dispatchEvent(new Event('loadeddata'));
  });
  await import('./flashThumbnail');
  await vi.waitFor(() => expect(window.__pictoFlashThumbnail).toEqual({ ready: true, width: 400, height: 200 }));
  expect(resume).toHaveBeenCalledTimes(1);
});
