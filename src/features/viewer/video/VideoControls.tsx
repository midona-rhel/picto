import {
  IconPlayerPause, IconPlayerPlay, IconPlayerSkipBack, IconPlayerSkipForward,
  IconRepeat, IconRepeatOff, IconMaximize,
} from '@tabler/icons-react';
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
}

export function VideoControls({ state, actions, onSeekStart, onSeekEnd, onToggleFullscreen }: Props) {
  return (
    <div className={styles.controls} onClick={(e) => e.stopPropagation()}>
      <ProgressBar currentTime={state.currentTime} duration={state.duration} buffered={state.buffered}
        onSeek={actions.seek} onSeekStart={onSeekStart} onSeekEnd={onSeekEnd} />
      <div className={styles.buttonRow}>
        <div className={styles.buttonRowLeft}>
          <button className={styles.icBtn} onClick={(e) => { e.stopPropagation(); actions.togglePlay(); }}
            title={state.isPlaying ? 'Pause' : 'Play'}>
            {state.isPlaying ? <IconPlayerPause size={20} /> : <IconPlayerPlay size={20} />}
          </button>
          <button className={styles.icBtn} onClick={(e) => { e.stopPropagation(); actions.seek(Math.max(0, state.currentTime - SKIP_STEP)); }}
            title={`Skip back ${SKIP_STEP}s`}>
            <IconPlayerSkipBack size={18} />
          </button>
          <button className={styles.icBtn} onClick={(e) => { e.stopPropagation(); actions.seek(Math.min(state.duration, state.currentTime + SKIP_STEP)); }}
            title={`Skip forward ${SKIP_STEP}s`}>
            <IconPlayerSkipForward size={18} />
          </button>
          <div className={styles.separator} />
          <span className={styles.timeDisplay}>{formatTime(state.currentTime)} / {formatTime(state.duration)}</span>
        </div>
        <div className={styles.buttonRowRight}>
          <PlaybackRateMenu rate={state.playbackRate} onRateChange={actions.setPlaybackRate} />
          <button className={`${styles.icBtn} ${state.loop ? styles.icBtnActive : ''}`}
            onClick={(e) => { e.stopPropagation(); actions.toggleLoop(); }}
            title={state.loop ? 'Loop on' : 'Loop off'}>
            {state.loop ? <IconRepeat size={18} /> : <IconRepeatOff size={18} />}
          </button>
          <VolumePanel volume={state.volume} muted={state.muted} onVolumeChange={actions.setVolume} onMuteToggle={actions.toggleMute} />
          {onToggleFullscreen && (
            <button className={styles.icBtn} onClick={(e) => { e.stopPropagation(); onToggleFullscreen(); }} title="Fullscreen">
              <IconMaximize size={18} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
