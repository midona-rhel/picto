import {
  IconPlayerPause, IconPlayerPlay, IconPlayerSkipBack, IconPlayerSkipForward,
  IconRepeat, IconRepeatOff, IconMaximize,
} from '@tabler/icons-react';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import { ProgressBar } from './ProgressBar';
import { VolumePanel } from './VolumePanel';
import { PlaybackRateMenu } from './PlaybackRateMenu';
import { formatTime } from './videoTimeFormat';
import { SKIP_STEP } from './videoConstants';
import type { VideoPlayerState, VideoPlayerActions } from './useVideoPlayer';
import styles from './VideoPlayer.module.css';

interface Props {
  state: VideoPlayerState;
  actions: VideoPlayerActions;
  onSeekStart?: () => void;
  onSeekEnd?: () => void;
  onToggleFullscreen?: () => void;
  waveformSrc?: string;
}

export function VideoControls({ state, actions, onSeekStart, onSeekEnd, onToggleFullscreen, waveformSrc }: Props) {
  return (
    <div className={styles.controls} onClick={(e) => e.stopPropagation()}>
      <ProgressBar currentTime={state.currentTime} duration={state.duration} buffered={state.buffered}
        onSeek={actions.seek} onSeekStart={onSeekStart} onSeekEnd={onSeekEnd} waveformSrc={waveformSrc} />
      <div className={styles.buttonRow}>
        <div className={styles.buttonRowLeft}>
          <KbdTooltip label={state.isPlaying ? 'Pause' : 'Play'} shortcutId="video.togglePlay">
            <button className={styles.icBtn} onClick={(e) => { e.stopPropagation(); actions.togglePlay(); }}>
              {state.isPlaying ? <IconPlayerPause size={20} /> : <IconPlayerPlay size={20} />}
            </button>
          </KbdTooltip>
          <KbdTooltip label={`Skip back ${SKIP_STEP}s`} shortcutId="video.seekBackward">
            <button className={styles.icBtn} onClick={(e) => { e.stopPropagation(); actions.seek(Math.max(0, state.currentTime - SKIP_STEP)); }}>
              <IconPlayerSkipBack size={18} />
            </button>
          </KbdTooltip>
          <KbdTooltip label={`Skip forward ${SKIP_STEP}s`} shortcutId="video.seekForward">
            <button className={styles.icBtn} onClick={(e) => { e.stopPropagation(); actions.seek(Math.min(state.duration, state.currentTime + SKIP_STEP)); }}>
              <IconPlayerSkipForward size={18} />
            </button>
          </KbdTooltip>
          <div className={styles.separator} />
          <span className={styles.timeDisplay}>{formatTime(state.currentTime)} / {formatTime(state.duration)}</span>
        </div>
        <div className={styles.buttonRowRight}>
          <PlaybackRateMenu rate={state.playbackRate} onRateChange={actions.setPlaybackRate} />
          <KbdTooltip label={state.loop ? 'Loop on' : 'Loop off'} shortcutId="video.toggleLoop">
            <button className={`${styles.icBtn} ${state.loop ? styles.icBtnActive : ''}`}
              onClick={(e) => { e.stopPropagation(); actions.toggleLoop(); }}>
              {state.loop ? <IconRepeat size={18} /> : <IconRepeatOff size={18} />}
            </button>
          </KbdTooltip>
          <VolumePanel volume={state.volume} muted={state.muted} onVolumeChange={actions.setVolume} onMuteToggle={actions.toggleMute} />
          {onToggleFullscreen && (
            <KbdTooltip label="Fullscreen" shortcutId="video.fullscreen">
              <button className={styles.icBtn} onClick={(e) => { e.stopPropagation(); onToggleFullscreen(); }}>
                <IconMaximize size={18} />
              </button>
            </KbdTooltip>
          )}
        </div>
      </div>
    </div>
  );
}
