import { useCallback, useEffect, useRef, useState } from 'react';
import { VideoControls } from './VideoControls';
import { VolumeHUD } from './VolumeHUD';
import { useVideoPlayer, type UseVideoPlayerOptions } from './useVideoPlayer';
import { CONTROLS_HIDE_DELAY, VOLUME_STEP } from './videoConstants';
import { getShortcut, matchesShortcutDef } from '../../../shared/lib/shortcuts';
import { useAudioVisualizationMode } from '../../../shared/lib/audioVisualization';
import { AudioVisualizer } from './AudioVisualizer';
import { captureVideoFrame, type CurrentFrameCapture } from '../currentFrameCapture';
import { useShortcutScope } from '../../../shared/hooks/useShortcutScope';
import styles from './VideoPlayer.module.css';

export interface VideoPlayerProps {
  src: string;
  kind?: 'video' | 'audio';
  waveformSrc?: string;
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
  onFrameCaptureChange?: (capture: CurrentFrameCapture | null) => void;
  onReady?: () => void;
  keyboardShortcutsEnabled?: boolean;
}

export function VideoPlayer({
  src, kind = 'video', waveformSrc, autoPlay = true, loop = true, muted = true,
  initialVolume = 0.9, initialPlaybackRate = 1.0,
  onEnded, onVolumeChange, onMutedChange, onPlaybackRateChange, onLoopChange,
  onFrameCaptureChange, keyboardShortcutsEnabled = true,
  onReady,
}: VideoPlayerProps) {
  const isAudio = kind === 'audio';
  const containerRef = useRef<HTMLDivElement>(null);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [seeking, setSeeking] = useState(false);
  const [volumeHudTrigger, setVolumeHudTrigger] = useState(0);
  const hideTimerRef = useRef<ReturnType<typeof setTimeout>>();

  const opts: UseVideoPlayerOptions = { autoPlay, loop, muted, initialVolume, initialPlaybackRate, onVolumeChange, onMutedChange, onPlaybackRateChange, onLoopChange };
  const { videoRef, state, actions } = useVideoPlayer(opts);
  const audioVisualization = useAudioVisualizationMode();

  const captureFrame = useCallback<CurrentFrameCapture>(async () => {
    const video = videoRef.current;
    if (!video) throw new Error('The video player is not ready.');
    return captureVideoFrame(video);
  }, [videoRef]);

  useEffect(() => {
    onFrameCaptureChange?.(isAudio ? null : captureFrame);
    return () => onFrameCaptureChange?.(null);
  }, [captureFrame, isAudio, onFrameCaptureChange]);

  const resetHideTimer = useCallback(() => {
    setControlsVisible(true);
    clearTimeout(hideTimerRef.current);
    hideTimerRef.current = setTimeout(() => { if (!seeking) setControlsVisible(false); }, CONTROLS_HIDE_DELAY);
  }, [seeking]);

  useEffect(() => () => clearTimeout(hideTimerRef.current), []);
  useEffect(() => {
    if (!state.isPlaying) { setControlsVisible(true); clearTimeout(hideTimerRef.current); }
    else resetHideTimer();
  }, [state.isPlaying]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleClick = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest(`.${styles.controls}`)) return;
    actions.togglePlay(); resetHideTimer();
  }, [actions, resetHideTimer]);

  const handleDoubleClick = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest(`.${styles.controls}`)) return;
    if (isAudio) return;
    e.preventDefault();
    const c = containerRef.current; if (!c) return;
    document.fullscreenElement ? document.exitFullscreen().catch(() => {}) : c.requestFullscreen().catch(() => {});
  }, [isAudio]);

  const handleToggleFullscreen = useCallback(() => {
    const c = containerRef.current; if (!c) return;
    document.fullscreenElement ? document.exitFullscreen().catch(() => {}) : c.requestFullscreen().catch(() => {});
  }, []);

  useEffect(() => {
    const v = videoRef.current; if (!v || !onEnded) return;
    v.addEventListener('ended', onEnded);
    return () => v.removeEventListener('ended', onEnded);
  }, [videoRef, onEnded]);

  // Keyboard shortcuts — uses registry for EU alternative keys
  useShortcutScope((e) => {
    const playDef = getShortcut('video.togglePlay')!;
    const seekBackwardDef = getShortcut('video.seekBackward')!;
    const seekForwardDef = getShortcut('video.seekForward')!;
    const volUpDef = getShortcut('video.volumeUp')!;
    const volDownDef = getShortcut('video.volumeDown')!;
    const muteDef = getShortcut('video.toggleMute')!;
    const loopDef = getShortcut('video.toggleLoop')!;
    const rateUpDef = getShortcut('video.rateIncrease')!;
    const rateDownDef = getShortcut('video.rateDecrease')!;

      if (matchesShortcutDef(e, playDef)) { e.preventDefault(); actions.togglePlay(); resetHideTimer(); return; }
      if (matchesShortcutDef(e, seekBackwardDef)) { e.preventDefault(); actions.seekRelative(-5); resetHideTimer(); return; }
      if (matchesShortcutDef(e, seekForwardDef)) { e.preventDefault(); actions.seekRelative(5); resetHideTimer(); return; }
      if (matchesShortcutDef(e, volUpDef)) { e.preventDefault(); actions.setVolume(state.volume + VOLUME_STEP); setVolumeHudTrigger(Date.now()); return; }
      if (matchesShortcutDef(e, volDownDef)) { e.preventDefault(); actions.setVolume(state.volume - VOLUME_STEP); setVolumeHudTrigger(Date.now()); return; }
      if (matchesShortcutDef(e, muteDef)) { e.preventDefault(); actions.toggleMute(); return; }
      if (matchesShortcutDef(e, loopDef)) { e.preventDefault(); actions.toggleLoop(); return; }
      if (matchesShortcutDef(e, rateUpDef)) { e.preventDefault(); actions.cyclePlaybackRate(1); return; }
      if (matchesShortcutDef(e, rateDownDef)) { e.preventDefault(); actions.cyclePlaybackRate(-1); return; }
  }, { enabled: keyboardShortcutsEnabled, priority: 70 });

  const showControls = isAudio || controlsVisible || !state.isPlaying || seeking;

  return (
    <div ref={containerRef} className={`${styles.root} ${isAudio ? styles.audioRoot : ''}`}
      onClick={handleClick} onDoubleClick={handleDoubleClick}
      onMouseMove={resetHideTimer} onMouseEnter={resetHideTimer}>
      {!isAudio && (
        <video ref={videoRef as React.RefObject<HTMLVideoElement>} src={src}
          autoPlay={autoPlay} loop={loop} muted={muted} playsInline tabIndex={-1} className={styles.video}
          onLoadedData={onReady} onError={onReady} />
      )}
      {isAudio && (
        <audio ref={videoRef as unknown as React.RefObject<HTMLAudioElement>}
          src={src} autoPlay={autoPlay} loop={loop} muted={muted} tabIndex={-1}
          onLoadedMetadata={onReady} onError={onReady} />
      )}
      {isAudio && (
        <AudioVisualizer
          mediaRef={videoRef as unknown as React.RefObject<HTMLMediaElement | null>}
          mode={audioVisualization}
        />
      )}
      <div className={showControls ? '' : styles.controlsHidden}>
        <VideoControls state={state} actions={actions}
          onSeekStart={() => setSeeking(true)} onSeekEnd={() => setSeeking(false)}
          onToggleFullscreen={isAudio ? undefined : handleToggleFullscreen}
          waveformSrc={isAudio ? waveformSrc : undefined} />
      </div>
      <VolumeHUD volume={state.volume} muted={state.muted} trigger={volumeHudTrigger} />
    </div>
  );
}
