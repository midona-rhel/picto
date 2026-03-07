/**
 * VideoPlayer — main video player component with custom controls.
 *
 * Renders <video> filling container + <VideoControls> overlay at bottom.
 * Handles auto-hide, click-to-toggle, double-click-fullscreen, scroll-to-seek/volume.
 * Arrow keys are NOT handled here — they navigate between files in DetailWindow.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { VideoControls } from './VideoControls';
import { VolumeHUD } from './VolumeHUD';
import { useVideoPlayer, type UseVideoPlayerOptions } from './useVideoPlayer';
import {
  CONTROLS_HIDE_DELAY,
  SEEK_SCROLL_STEP,
  VOLUME_SCROLL_STEP,
  VOLUME_STEP,
} from './videoConstants';
import { getShortcut, matchesShortcutDef } from '../../../shared/lib/shortcuts';
import { useGlobalKeydown } from '../../../shared/hooks/useGlobalKeydown';
import styles from './VideoPlayer.module.css';

export interface VideoPlayerProps {
  src: string;
  autoPlay?: boolean;
  loop?: boolean;
  muted?: boolean;
  initialVolume?: number;
  initialPlaybackRate?: number;
  onEnded?: () => void;
  onVolumeChange?: (volume: number) => void;
  onMutedChange?: (muted: boolean) => void;
  onPlaybackRateChange?: (rate: number) => void;
  onLoopChange?: (loop: boolean) => void;
  className?: string;
  /** CSS transform applied to the <video> element (e.g. rotate/flip from toolbar) */
  videoTransform?: string;
}

export function VideoPlayer({
  src,
  autoPlay = true,
  loop = true,
  muted = true,
  initialVolume = 0.9,
  initialPlaybackRate = 1.0,
  onEnded,
  onVolumeChange,
  onMutedChange,
  onPlaybackRateChange,
  onLoopChange,
  className,
  videoTransform,
}: VideoPlayerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [seeking, setSeeking] = useState(false);
  const [volumeHudTrigger, setVolumeHudTrigger] = useState(0);
  const hideTimerRef = useRef<ReturnType<typeof setTimeout>>();

  const options: UseVideoPlayerOptions = {
    autoPlay,
    loop,
    muted,
    initialVolume,
    initialPlaybackRate,
    onVolumeChange,
    onMutedChange,
    onPlaybackRateChange,
    onLoopChange,
  };

  const { videoRef, state, actions } = useVideoPlayer(options);

  // Auto-hide controls
  const resetHideTimer = useCallback(() => {
    setControlsVisible(true);
    clearTimeout(hideTimerRef.current);
    hideTimerRef.current = setTimeout(() => {
      if (!seeking) setControlsVisible(false);
    }, CONTROLS_HIDE_DELAY);
  }, [seeking]);

  useEffect(() => {
    return () => clearTimeout(hideTimerRef.current);
  }, []);

  // Show controls when paused
  useEffect(() => {
    if (!state.isPlaying) {
      setControlsVisible(true);
      clearTimeout(hideTimerRef.current);
    } else {
      resetHideTimer();
    }
  }, [state.isPlaying]); // eslint-disable-line react-hooks/exhaustive-deps

  // Click to toggle play
  const handleClick = useCallback((e: React.MouseEvent) => {
    // Don't toggle if clicking on controls area
    if ((e.target as HTMLElement).closest(`.${styles.controls}`)) return;
    actions.togglePlay();
    resetHideTimer();
  }, [actions, resetHideTimer]);

  // Double-click for fullscreen
  const handleDoubleClick = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest(`.${styles.controls}`)) return;
    e.preventDefault();
    const container = containerRef.current;
    if (!container) return;
    if (document.fullscreenElement) {
      document.exitFullscreen().catch(() => {});
    } else {
      container.requestFullscreen().catch(() => {});
    }
  }, []);

  // Toggle fullscreen (for button)
  const handleToggleFullscreen = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    if (document.fullscreenElement) {
      document.exitFullscreen().catch(() => {});
    } else {
      container.requestFullscreen().catch(() => {});
    }
  }, []);

  // Scroll: seek by default, volume when Alt held
  const handleWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    if (e.altKey) {
      // Volume
      const delta = e.deltaY < 0 ? VOLUME_SCROLL_STEP : -VOLUME_SCROLL_STEP;
      actions.setVolume(state.volume + delta);
      setVolumeHudTrigger(Date.now());
    } else {
      // Seek
      const delta = e.deltaY < 0 ? SEEK_SCROLL_STEP : -SEEK_SCROLL_STEP;
      actions.seekRelative(delta);
    }
    resetHideTimer();
  }, [actions, state.volume, resetHideTimer]);

  // Video ended callback
  useEffect(() => {
    const video = videoRef.current;
    if (!video || !onEnded) return;
    video.addEventListener('ended', onEnded);
    return () => video.removeEventListener('ended', onEnded);
  }, [videoRef, onEnded]);

  // Keyboard shortcuts — only non-arrow-key shortcuts
  // Arrow keys are reserved for file navigation (handled by DetailWindow)
  const handleVideoHotkeys = useCallback((e: KeyboardEvent) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

    const container = containerRef.current;
    if (!container) return;

    const togglePlayShortcut = getShortcut('video.togglePlay');
    const volumeUpShortcut = getShortcut('video.volumeUp');
    const volumeDownShortcut = getShortcut('video.volumeDown');
    const toggleMuteShortcut = getShortcut('video.toggleMute');
    const toggleLoopShortcut = getShortcut('video.toggleLoop');
    const rateIncreaseShortcut = getShortcut('video.rateIncrease');
    const rateDecreaseShortcut = getShortcut('video.rateDecrease');
    const rateResetShortcut = getShortcut('video.rateReset');

    if (togglePlayShortcut && matchesShortcutDef(e, togglePlayShortcut)) {
      e.preventDefault();
      actions.togglePlay();
      resetHideTimer();
      return;
    }
    if (volumeUpShortcut && matchesShortcutDef(e, volumeUpShortcut)) {
      e.preventDefault();
      actions.setVolume(state.volume + VOLUME_STEP);
      setVolumeHudTrigger(Date.now());
      resetHideTimer();
      return;
    }
    if (volumeDownShortcut && matchesShortcutDef(e, volumeDownShortcut)) {
      e.preventDefault();
      actions.setVolume(state.volume - VOLUME_STEP);
      setVolumeHudTrigger(Date.now());
      resetHideTimer();
      return;
    }
    if (toggleMuteShortcut && matchesShortcutDef(e, toggleMuteShortcut)) {
      e.preventDefault();
      actions.toggleMute();
      resetHideTimer();
      return;
    }
    if (toggleLoopShortcut && matchesShortcutDef(e, toggleLoopShortcut)) {
      e.preventDefault();
      actions.toggleLoop();
      resetHideTimer();
      return;
    }
    if (rateIncreaseShortcut && matchesShortcutDef(e, rateIncreaseShortcut)) {
      e.preventDefault();
      actions.cyclePlaybackRate(1);
      resetHideTimer();
      return;
    }
    if (rateDecreaseShortcut && matchesShortcutDef(e, rateDecreaseShortcut)) {
      e.preventDefault();
      actions.cyclePlaybackRate(-1);
      resetHideTimer();
      return;
    }
    if (rateResetShortcut && matchesShortcutDef(e, rateResetShortcut)) {
      e.preventDefault();
      actions.setPlaybackRate(1);
      resetHideTimer();
    }
  }, [actions, state.volume, resetHideTimer]);
  useGlobalKeydown(handleVideoHotkeys);

  const showControls = controlsVisible || !state.isPlaying || seeking;
  const videoStyle = useMemo(
    () => (videoTransform ? { transform: videoTransform } : undefined),
    [videoTransform],
  );

  return (
    <div
      ref={containerRef}
      className={`${styles.root} ${className ?? ''}`}
      onClick={handleClick}
      onDoubleClick={handleDoubleClick}
      onMouseMove={resetHideTimer}
      onMouseEnter={resetHideTimer}
      onWheel={handleWheel}
    >
      <video
        ref={videoRef as React.RefObject<HTMLVideoElement>}
        src={src}
        autoPlay={autoPlay}
        loop={loop}
        muted={muted}
        playsInline
        tabIndex={-1}
        className={styles.video}
        style={videoStyle}
      />

      {/* Controls overlay */}
      <div className={showControls ? '' : styles.controlsHidden}>
        <VideoControls
          state={state}
          actions={actions}
          onSeekStart={() => setSeeking(true)}
          onSeekEnd={() => setSeeking(false)}
          onToggleFullscreen={handleToggleFullscreen}
          src={src}
        />
      </div>

      {/* Volume HUD */}
      <VolumeHUD
        volume={state.volume}
        muted={state.muted}
        trigger={volumeHudTrigger}
      />
    </div>
  );
}
