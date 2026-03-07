/**
 * useVideoPlayer — core video state management hook.
 *
 * Wraps an HTML <video> element and provides a clean imperative API
 * for play/pause, seek, volume, playback rate, loop, and frame stepping.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { PLAYBACK_RATES, DEFAULT_FPS } from './videoConstants';
import { useFrameTime } from './useFrameTime';

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

export interface VideoPlayerState {
  isPlaying: boolean;
  duration: number;
  currentTime: number;
  volume: number;
  muted: boolean;
  playbackRate: number;
  loop: boolean;
  buffered: TimeRanges | null;
}

export interface VideoPlayerActions {
  play(): void;
  pause(): void;
  togglePlay(): void;
  seek(time: number): void;
  seekRelative(delta: number): void;
  stepFrame(direction: 1 | -1): void;
  setVolume(v: number): void;
  setMuted(m: boolean): void;
  toggleMute(): void;
  setPlaybackRate(r: number): void;
  cyclePlaybackRate(direction: 1 | -1): void;
  setLoop(l: boolean): void;
  toggleLoop(): void;
}

export interface UseVideoPlayerOptions {
  autoPlay?: boolean;
  loop?: boolean;
  muted?: boolean;
  initialVolume?: number;
  initialPlaybackRate?: number;
  onVolumeChange?: (volume: number) => void;
  onMutedChange?: (muted: boolean) => void;
  onPlaybackRateChange?: (rate: number) => void;
  onLoopChange?: (loop: boolean) => void;
}

export interface UseVideoPlayerResult {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  state: VideoPlayerState;
  actions: VideoPlayerActions;
  /** Frame-accurate time from requestVideoFrameCallback */
  frameTime: number;
  /** Detected FPS */
  fps: number;
}

export function useVideoPlayer(options: UseVideoPlayerOptions = {}): UseVideoPlayerResult {
  const {
    loop: initialLoop = true,
    muted: initialMuted = true,
    initialVolume = 0.9,
    initialPlaybackRate = 1.0,
    onVolumeChange,
    onMutedChange,
    onPlaybackRateChange,
    onLoopChange,
  } = options;

  const videoRef = useRef<HTMLVideoElement>(null);

  const [isPlaying, setIsPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [volume, setVolumeState] = useState(initialMuted ? initialVolume : initialVolume);
  const [muted, setMutedState] = useState(initialMuted);
  const [playbackRate, setPlaybackRateState] = useState(initialPlaybackRate);
  const [loop, setLoopState] = useState(initialLoop);
  const [buffered, setBuffered] = useState<TimeRanges | null>(null);

  // Seeking state — when true, don't update currentTime from timeupdate
  const seekingRef = useRef(false);

  // Frame time from requestVideoFrameCallback
  const { frameTime, fps } = useFrameTime(videoRef);

  // Sync video element properties when state changes
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.volume = clamp(volume, 0, 1);
  }, [volume]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.muted = muted;
  }, [muted]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.playbackRate = playbackRate;
  }, [playbackRate]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.loop = loop;
  }, [loop]);

  // Video event listeners
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onLoadedMetadata = () => {
      setDuration(Number.isFinite(video.duration) ? video.duration : 0);
      // Apply initial settings
      video.volume = clamp(volume, 0, 1);
      video.muted = muted;
      video.playbackRate = playbackRate;
      video.loop = loop;
    };
    const onTimeUpdate = () => {
      if (!seekingRef.current) {
        setCurrentTime(video.currentTime);
      }
    };
    const onProgress = () => {
      setBuffered(video.buffered);
    };
    const onSeeking = () => {
      seekingRef.current = true;
    };
    const onSeeked = () => {
      seekingRef.current = false;
      setCurrentTime(video.currentTime);
    };
    const onDurationChange = () => {
      setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    };

    video.addEventListener('play', onPlay);
    video.addEventListener('pause', onPause);
    video.addEventListener('loadedmetadata', onLoadedMetadata);
    video.addEventListener('timeupdate', onTimeUpdate);
    video.addEventListener('progress', onProgress);
    video.addEventListener('seeking', onSeeking);
    video.addEventListener('seeked', onSeeked);
    video.addEventListener('durationchange', onDurationChange);

    // If metadata already loaded (cached video)
    if (video.readyState >= 1) {
      onLoadedMetadata();
    }

    return () => {
      video.removeEventListener('play', onPlay);
      video.removeEventListener('pause', onPause);
      video.removeEventListener('loadedmetadata', onLoadedMetadata);
      video.removeEventListener('timeupdate', onTimeUpdate);
      video.removeEventListener('progress', onProgress);
      video.removeEventListener('seeking', onSeeking);
      video.removeEventListener('seeked', onSeeked);
      video.removeEventListener('durationchange', onDurationChange);
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Actions
  const play = useCallback(() => {
    videoRef.current?.play().catch(() => {});
  }, []);

  const pause = useCallback(() => {
    videoRef.current?.pause();
  }, []);

  const togglePlay = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      video.play().catch(() => {});
    } else {
      video.pause();
    }
  }, []);

  const seek = useCallback((time: number) => {
    const video = videoRef.current;
    if (!video) return;
    video.currentTime = clamp(time, 0, video.duration || 0);
    setCurrentTime(video.currentTime);
  }, []);

  const seekRelative = useCallback((delta: number) => {
    const video = videoRef.current;
    if (!video) return;
    video.currentTime = clamp(video.currentTime + delta, 0, video.duration || 0);
    setCurrentTime(video.currentTime);
  }, []);

  const stepFrame = useCallback((direction: 1 | -1) => {
    const video = videoRef.current;
    if (!video) return;
    video.pause();
    const frameDuration = 1 / (fps || DEFAULT_FPS);
    video.currentTime = clamp(
      video.currentTime + direction * frameDuration,
      0,
      video.duration || 0,
    );
    setCurrentTime(video.currentTime);
  }, [fps]);

  const setVolume = useCallback((v: number) => {
    const clamped = clamp(v, 0, 1);
    setVolumeState(clamped);
    if (clamped > 0) setMutedState(false);
    onVolumeChange?.(clamped);
    if (clamped > 0) onMutedChange?.(false);
  }, [onVolumeChange, onMutedChange]);

  const setMuted = useCallback((m: boolean) => {
    setMutedState(m);
    onMutedChange?.(m);
  }, [onMutedChange]);

  const toggleMute = useCallback(() => {
    setMutedState((prev) => {
      const next = !prev;
      onMutedChange?.(next);
      return next;
    });
  }, [onMutedChange]);

  const setPlaybackRate = useCallback((r: number) => {
    setPlaybackRateState(r);
    onPlaybackRateChange?.(r);
  }, [onPlaybackRateChange]);

  const cyclePlaybackRate = useCallback((direction: 1 | -1) => {
    setPlaybackRateState((current) => {
      const idx = PLAYBACK_RATES.indexOf(current as typeof PLAYBACK_RATES[number]);
      let nextIdx: number;
      if (idx === -1) {
        // Find nearest
        nextIdx = PLAYBACK_RATES.findIndex((r) => r >= current);
        if (nextIdx === -1) nextIdx = PLAYBACK_RATES.length - 1;
      } else {
        nextIdx = clamp(idx + direction, 0, PLAYBACK_RATES.length - 1);
      }
      const next = PLAYBACK_RATES[nextIdx];
      onPlaybackRateChange?.(next);
      return next;
    });
  }, [onPlaybackRateChange]);

  const setLoop = useCallback((l: boolean) => {
    setLoopState(l);
    onLoopChange?.(l);
  }, [onLoopChange]);

  const toggleLoop = useCallback(() => {
    setLoopState((prev) => {
      const next = !prev;
      onLoopChange?.(next);
      return next;
    });
  }, [onLoopChange]);

  const state: VideoPlayerState = {
    isPlaying,
    duration,
    currentTime,
    volume,
    muted,
    playbackRate,
    loop,
    buffered,
  };

  const actions: VideoPlayerActions = {
    play,
    pause,
    togglePlay,
    seek,
    seekRelative,
    stepFrame,
    setVolume,
    setMuted,
    toggleMute,
    setPlaybackRate,
    cyclePlaybackRate,
    setLoop,
    toggleLoop,
  };

  return { videoRef, state, actions, frameTime, fps };
}
