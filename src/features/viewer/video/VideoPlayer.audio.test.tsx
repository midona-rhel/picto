import { fireEvent, render } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { VideoPlayer } from './VideoPlayer';
import { resetShortcutRuntimeForTests } from '../../../runtime/shortcutRuntime';

const actions = vi.hoisted(() => ({
  play: vi.fn(), pause: vi.fn(), togglePlay: vi.fn(), seek: vi.fn(), seekRelative: vi.fn(),
  stepFrame: vi.fn(), setVolume: vi.fn(), toggleMute: vi.fn(), setPlaybackRate: vi.fn(),
  cyclePlaybackRate: vi.fn(), toggleLoop: vi.fn(),
}));

vi.mock('../../../shared/ui/KbdTooltip', () => ({
  KbdTooltip: ({ children, label, shortcutId }: { children: ReactNode; label: string; shortcutId?: string }) => (
    <span data-tooltip-label={label} data-tooltip-shortcut-id={shortcutId ?? ''}>{children}</span>
  ),
}));

vi.mock('./useVideoPlayer', () => ({
  useVideoPlayer: () => ({
    videoRef: { current: null },
    state: {
      isPlaying: false,
      duration: 120,
      currentTime: 30,
      volume: 0.9,
      muted: false,
      playbackRate: 1,
      loop: true,
      buffered: null,
    },
    actions,
  }),
}));

beforeEach(() => {
  vi.clearAllMocks();
  resetShortcutRuntimeForTests();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => null);
});

afterEach(() => {
  resetShortcutRuntimeForTests();
  vi.restoreAllMocks();
});

describe('VideoPlayer audio mode', () => {
  it('renders audio playback with a waveform progress surface and no video element', () => {
    const { container } = render(
      <VideoPlayer
        kind="audio"
        src="media://localhost/file/hash.mp3"
        waveformSrc="media://localhost/thumb/hash.jpg"
        muted={false}
      />,
    );

    expect(container.querySelector('audio')).toHaveAttribute('src', 'media://localhost/file/hash.mp3');
    expect(container.querySelector('audio')).not.toHaveAttribute('crossorigin');
    expect(container.querySelector('video')).toBeNull();
    expect(container.querySelector('[data-audio-visualization]')).toHaveAttribute('data-audio-visualization', 'spectrum');
    expect(container.querySelectorAll('[data-waveform-layer]')).toHaveLength(2);
    expect(container.querySelector('[data-waveform-layer="unplayed"]')).toHaveStyle({ backgroundColor: 'rgba(255, 255, 255, 0.2)' });
    expect(container.querySelector('[data-waveform-layer="played"]')).toHaveStyle({ backgroundColor: 'rgba(255, 255, 255, 0.95)' });
    expect(container.querySelectorAll('button')).toHaveLength(6);
  });

  it('uses the registered non-conflicting video transport keys', () => {
    const { container } = render(<VideoPlayer src="media://localhost/file/hash.mp4" />);

    fireEvent.keyDown(window, { key: ' ', code: 'Space' });
    expect(actions.togglePlay).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: 'p' });
    fireEvent.keyDown(window, { key: 'k' });
    expect(actions.togglePlay).toHaveBeenCalledOnce();

    fireEvent.keyDown(window, { key: 'j' });
    fireEvent.keyDown(window, { key: 'l' });
    expect(actions.seekRelative.mock.calls).toEqual([[-5], [5]]);
    expect(actions.toggleLoop).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: 'o' });
    expect(actions.toggleLoop).toHaveBeenCalledOnce();

    fireEvent.keyDown(window, { key: '\\' });
    expect(actions.setPlaybackRate).toHaveBeenCalledWith(1);

    expect(container.querySelector('[data-tooltip-label="Play"]')).toHaveAttribute('data-tooltip-shortcut-id', 'video.togglePlay');
    expect(container.querySelector('[data-tooltip-label="Skip back 5s"]')).toHaveAttribute('data-tooltip-shortcut-id', 'video.seekBackward');
    expect(container.querySelector('[data-tooltip-label="Skip forward 5s"]')).toHaveAttribute('data-tooltip-shortcut-id', 'video.seekForward');
    expect(container.querySelector('[data-tooltip-label="Loop on"]')).toHaveAttribute('data-tooltip-shortcut-id', 'video.toggleLoop');
    expect(container.querySelector('[data-tooltip-label="Mute"]')).toHaveAttribute('data-tooltip-shortcut-id', 'video.toggleMute');
    expect(container.querySelector('[data-tooltip-label="Fullscreen"]')).toHaveAttribute('data-tooltip-shortcut-id', 'video.fullscreen');
  });
});
