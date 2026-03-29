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
  toggleMute(): void;
  setPlaybackRate(r: number): void;
  cyclePlaybackRate(direction: 1 | -1): void;
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

export function useVideoPlayer(options: UseVideoPlayerOptions = {}) {
  const {
    loop: initialLoop = true, muted: initialMuted = true,
    initialVolume = 0.9, initialPlaybackRate = 1.0,
    onVolumeChange, onMutedChange, onPlaybackRateChange, onLoopChange,
  } = options;

  const videoRef = useRef<HTMLVideoElement>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [volume, setVolumeState] = useState(initialVolume);
  const [muted, setMutedState] = useState(initialMuted);
  const [playbackRate, setPlaybackRateState] = useState(initialPlaybackRate);
  const [loop, setLoopState] = useState(initialLoop);
  const [buffered, setBuffered] = useState<TimeRanges | null>(null);
  const seekingRef = useRef(false);
  const { frameTime, fps } = useFrameTime(videoRef);

  useEffect(() => { const v = videoRef.current; if (v) v.volume = clamp(volume, 0, 1); }, [volume]);
  useEffect(() => { const v = videoRef.current; if (v) v.muted = muted; }, [muted]);
  useEffect(() => { const v = videoRef.current; if (v) v.playbackRate = playbackRate; }, [playbackRate]);
  useEffect(() => { const v = videoRef.current; if (v) v.loop = loop; }, [loop]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onMeta = () => { setDuration(Number.isFinite(video.duration) ? video.duration : 0); video.volume = clamp(volume, 0, 1); video.muted = muted; video.playbackRate = playbackRate; video.loop = loop; };
    const onTime = () => { if (!seekingRef.current) setCurrentTime(video.currentTime); };
    const onProgress = () => setBuffered(video.buffered);
    const onSeeking = () => { seekingRef.current = true; };
    const onSeeked = () => { seekingRef.current = false; setCurrentTime(video.currentTime); };
    const onDur = () => setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    video.addEventListener('play', onPlay); video.addEventListener('pause', onPause);
    video.addEventListener('loadedmetadata', onMeta); video.addEventListener('timeupdate', onTime);
    video.addEventListener('progress', onProgress); video.addEventListener('seeking', onSeeking);
    video.addEventListener('seeked', onSeeked); video.addEventListener('durationchange', onDur);
    if (video.readyState >= 1) onMeta();
    return () => { video.removeEventListener('play', onPlay); video.removeEventListener('pause', onPause); video.removeEventListener('loadedmetadata', onMeta); video.removeEventListener('timeupdate', onTime); video.removeEventListener('progress', onProgress); video.removeEventListener('seeking', onSeeking); video.removeEventListener('seeked', onSeeked); video.removeEventListener('durationchange', onDur); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const play = useCallback(() => { videoRef.current?.play().catch(() => {}); }, []);
  const pause = useCallback(() => { videoRef.current?.pause(); }, []);
  const togglePlay = useCallback(() => { const v = videoRef.current; if (!v) return; v.paused ? v.play().catch(() => {}) : v.pause(); }, []);
  const seek = useCallback((t: number) => { const v = videoRef.current; if (!v) return; v.currentTime = clamp(t, 0, v.duration || 0); setCurrentTime(v.currentTime); }, []);
  const seekRelative = useCallback((d: number) => { const v = videoRef.current; if (!v) return; v.currentTime = clamp(v.currentTime + d, 0, v.duration || 0); setCurrentTime(v.currentTime); }, []);
  const stepFrame = useCallback((dir: 1 | -1) => { const v = videoRef.current; if (!v) return; v.pause(); v.currentTime = clamp(v.currentTime + dir / (fps || DEFAULT_FPS), 0, v.duration || 0); setCurrentTime(v.currentTime); }, [fps]);
  const setVolume = useCallback((val: number) => { const c = clamp(val, 0, 1); setVolumeState(c); if (c > 0) setMutedState(false); onVolumeChange?.(c); if (c > 0) onMutedChange?.(false); }, [onVolumeChange, onMutedChange]);
  const toggleMute = useCallback(() => { setMutedState(p => { const n = !p; onMutedChange?.(n); return n; }); }, [onMutedChange]);
  const setPlaybackRate = useCallback((r: number) => { setPlaybackRateState(r); onPlaybackRateChange?.(r); }, [onPlaybackRateChange]);
  const cyclePlaybackRate = useCallback((dir: 1 | -1) => { setPlaybackRateState(cur => { const idx = PLAYBACK_RATES.indexOf(cur as any); let next = idx === -1 ? PLAYBACK_RATES.findIndex(r => r >= cur) : clamp(idx + dir, 0, PLAYBACK_RATES.length - 1); if (next === -1) next = PLAYBACK_RATES.length - 1; const r = PLAYBACK_RATES[next]; onPlaybackRateChange?.(r); return r; }); }, [onPlaybackRateChange]);
  const toggleLoop = useCallback(() => { setLoopState(p => { const n = !p; onLoopChange?.(n); return n; }); }, [onLoopChange]);

  return {
    videoRef,
    state: { isPlaying, duration, currentTime, volume, muted, playbackRate, loop, buffered } as VideoPlayerState,
    actions: { play, pause, togglePlay, seek, seekRelative, stepFrame, setVolume, toggleMute, setPlaybackRate, cyclePlaybackRate, toggleLoop } as VideoPlayerActions,
    frameTime, fps,
  };
}
