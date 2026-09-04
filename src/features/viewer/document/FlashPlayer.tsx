import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent } from 'react';
import styles from './FlashPlayer.module.css';
import type { CurrentFrameCapture } from '../currentFrameCapture';
import { windowController } from '../../../controllers/windowController';
import {
  applyPictoRuffleChrome,
  createRufflePlayer,
  loadRuffleMovie,
  type RufflePlayerElement,
} from '../../../shared/flash/ruffleRuntime';
import { useShortcutScope, useShortcutSuspension } from '../../../shared/hooks/useShortcutScope';
import { t } from '../../../i18n';
import { fitFlashStage, type FlashStageSize } from './flashStageGeometry';
import { FlashControls } from './FlashControls';
import { useMediaControlsVisibility } from '../video/useMediaControlsVisibility';
import { getShortcut, matchesShortcutDef } from '../../../shared/lib/shortcuts';
import { VOLUME_STEP } from '../video/videoConstants';
import { reportPlaybackFailure } from '../video/mediaPlaybackDiagnostics';

export interface FlashPlaybackController {
  isPlaying: boolean;
  muted: boolean;
  volume: number;
  togglePlay(): void;
  stop(): void;
  toggleMute(): void;
  setVolume(volume: number): void;
}

interface FlashPlayerProps {
  src: string;
  onPlaybackChange?: (controller: FlashPlaybackController | null) => void;
  onContextMenu?: (event: MouseEvent) => void;
  onFrameCaptureChange?: (capture: CurrentFrameCapture | null) => void;
  onReady?: () => void;
}

export function FlashPlayer({ src, onPlaybackChange, onContextMenu, onFrameCaptureChange, onReady }: FlashPlayerProps) {
  const stageRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const playerRef = useRef<RufflePlayerElement | null>(null);
  const [status, setStatus] = useState<'loading' | 'ready' | 'stopped'>('loading');
  const [isPlaying, setIsPlaying] = useState(false);
  const [volume, setVolumeState] = useState(1);
  const [muted, setMuted] = useState(false);
  const [hasInputFocus, setHasInputFocus] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [viewportSize, setViewportSize] = useState<FlashStageSize>({ width: 0, height: 0 });
  const [movieSize, setMovieSize] = useState<FlashStageSize | null>(null);
  const { controlsVisible, revealControls } = useMediaControlsVisibility();
  useShortcutSuspension(hasInputFocus);

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const updateSize = (width: number, height: number) => {
      setViewportSize((current) => current.width === width && current.height === height
        ? current
        : { width, height });
    };
    const bounds = stage.getBoundingClientRect();
    updateSize(bounds.width, bounds.height);
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(([entry]) => {
      updateSize(entry.contentRect.width, entry.contentRect.height);
    });
    observer.observe(stage);
    return () => observer.disconnect();
  }, []);

  const fittedStage = useMemo(() => fitFlashStage(viewportSize, movieSize), [movieSize, viewportSize]);

  const captureFrame = useCallback<CurrentFrameCapture>(async () => {
    const bounds = hostRef.current?.getBoundingClientRect();
    if (!bounds || bounds.width <= 0 || bounds.height <= 0) {
      throw new Error('The Flash frame is not ready yet.');
    }
    const controls = document.querySelector<HTMLElement>('[data-flash-controls]');
    const previousVisibility = controls?.style.visibility ?? '';
    if (controls) controls.style.visibility = 'hidden';
    try {
      await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      return windowController.captureCurrentWindowRect({
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
      });
    } finally {
      if (controls) controls.style.visibility = previousVisibility;
    }
  }, []);

  useEffect(() => {
    onFrameCaptureChange?.(captureFrame);
    return () => onFrameCaptureChange?.(null);
  }, [captureFrame, onFrameCaptureChange]);

  const togglePlay = useCallback(() => {
    revealControls();
    const player = playerRef.current;
    if (!player) return;
    const runtime = player.ruffle(1);
    if (!runtime.suspended) {
      runtime.suspend();
      setIsPlaying(false);
    } else {
      runtime.resume();
      setStatus('ready');
      setIsPlaying(true);
    }
  }, [revealControls]);

  const stop = useCallback(() => {
    revealControls();
    const player = playerRef.current;
    if (!player) return;
    player.ruffle(1).suspend();
    setStatus('stopped');
    setIsPlaying(false);
    void loadRuffleMovie(player, src, 'off').then(() => player.ruffle(1).suspend()).catch((reason: unknown) => {
      reportPlaybackFailure(src, { code: 'RUFFLE_RESET_ERROR', message: String(reason) });
    });
  }, [revealControls, src]);

  const setVolume = useCallback((nextVolume: number) => {
    revealControls();
    const clamped = Math.min(1, Math.max(0, nextVolume));
    setVolumeState(clamped);
    if (clamped > 0) setMuted(false);
    if (playerRef.current) playerRef.current.ruffle(1).volume = clamped;
  }, [revealControls]);

  const toggleMute = useCallback(() => {
    revealControls();
    setMuted((wasMuted) => {
      if (playerRef.current) playerRef.current.ruffle(1).volume = wasMuted ? volume : 0;
      return !wasMuted;
    });
  }, [revealControls, volume]);

  const controller = useMemo<FlashPlaybackController>(() => ({
    isPlaying,
    muted,
    volume,
    togglePlay,
    stop,
    toggleMute,
    setVolume,
  }), [isPlaying, muted, setVolume, stop, toggleMute, togglePlay, volume]);

  useShortcutScope((event) => {
    if (matchesShortcutDef(event, getShortcut('video.togglePlay')!)) {
      event.preventDefault();
      togglePlay();
      return;
    }
    if (matchesShortcutDef(event, getShortcut('video.volumeUp')!)) {
      event.preventDefault();
      setVolume(volume + VOLUME_STEP);
      return;
    }
    if (matchesShortcutDef(event, getShortcut('video.volumeDown')!)) {
      event.preventDefault();
      setVolume(volume - VOLUME_STEP);
      return;
    }
    if (matchesShortcutDef(event, getShortcut('video.toggleMute')!)) {
      event.preventDefault();
      toggleMute();
    }
  }, { priority: 70 });

  useEffect(() => {
    onPlaybackChange?.(controller);
  }, [controller, onPlaybackChange]);

  useEffect(() => () => onPlaybackChange?.(null), [onPlaybackChange]);

  useEffect(() => {
    let disposed = false;
    let player: RufflePlayerElement | null = null;
    setStatus('loading');
    setIsPlaying(false);
    setError(null);
    setMovieSize(null);

    void createRufflePlayer().then(async (createdPlayer) => {
      if (disposed || !hostRef.current) return;
      player = createdPlayer;
      playerRef.current = player;
      player.className = styles.player;
      player.ruffle(1).volume = muted ? 0 : volume;
      hostRef.current.replaceChildren(player);
      applyPictoRuffleChrome(player);
      let ready = false;
      const markReady = () => {
        if (disposed || ready) return;
        ready = true;
        const metadata = player?.ruffle(1).metadata;
        if (metadata?.width && metadata?.height) {
          setMovieSize({ width: metadata.width, height: metadata.height });
        }
        requestAnimationFrame(() => requestAnimationFrame(() => {
          if (!disposed) {
            setStatus('ready');
            onReady?.();
          }
        }));
        setIsPlaying(true);
      };
      player.addEventListener('loadeddata', markReady, { once: true });
      await loadRuffleMovie(player, src, 'on');
      if (!ready && player.ruffle(1).readyState === 2) setTimeout(markReady, 120);
    }).catch((reason: unknown) => {
      if (!disposed) {
        reportPlaybackFailure(src, { code: 'RUFFLE_LOAD_ERROR', message: String(reason) });
        setError(reason instanceof Error ? reason.message : t('Could not open this Flash file.'));
        onReady?.();
      }
    });

    return () => {
      disposed = true;
      playerRef.current = null;
      player?.remove();
      hostRef.current?.replaceChildren();
    };
  }, [onReady, src]); // Playback state is applied imperatively; loading is owned by the source.

  return (
    <div
      ref={stageRef}
      className={`${styles.stage} ${status === 'ready' ? styles.ready : ''}`}
      data-flash-player
      data-status={error ? 'error' : status}
      tabIndex={0}
      onContextMenu={onContextMenu}
      onKeyDownCapture={(event) => {
        const mediaShortcutIds = [
          'video.togglePlay', 'video.volumeUp', 'video.volumeDown', 'video.toggleMute',
        ];
        if (mediaShortcutIds.some((id) => matchesShortcutDef(event.nativeEvent, getShortcut(id)!))) revealControls();
      }}
      onMouseMove={revealControls}
      onMouseEnter={revealControls}
      onPointerDownCapture={(event) => {
        revealControls();
        event.currentTarget.focus();
        setHasInputFocus(true);
      }}
      onFocusCapture={() => setHasInputFocus(true)}
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setHasInputFocus(false);
        }
      }}
    >
      <div
        ref={hostRef}
        className={styles.host}
        data-flash-stage
        style={fittedStage ? { width: fittedStage.width, height: fittedStage.height } : undefined}
      />
      <FlashControls controller={controller} visible={controlsVisible} />
      {error ? <div className={styles.message} role="alert">{error}</div> : null}
    </div>
  );
}
