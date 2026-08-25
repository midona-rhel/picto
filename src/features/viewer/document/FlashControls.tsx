import { IconPlayerPause, IconPlayerPlay, IconPlayerStop } from '@tabler/icons-react';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import { VolumePanel } from '../video/VolumePanel';
import videoStyles from '../video/VideoPlayer.module.css';
import type { FlashPlaybackController } from './FlashPlayer';
import styles from './FlashControls.module.css';

interface FlashControlsProps {
  controller: FlashPlaybackController | null;
}

export function FlashControls({ controller }: FlashControlsProps) {
  const isPlaying = controller?.isPlaying ?? false;

  return (
    <div
      className={`${videoStyles.controls} ${styles.controls}`}
      data-flash-controls
      onMouseDown={(event) => event.stopPropagation()}
      onClick={(event) => event.stopPropagation()}
    >
      <div className={videoStyles.buttonRow}>
        <div className={videoStyles.buttonRowLeft}>
          <KbdTooltip label={isPlaying ? 'Pause' : 'Play'}>
            <button
              className={videoStyles.icBtn}
              aria-label={isPlaying ? 'Pause Flash content' : 'Play Flash content'}
              disabled={!controller}
              onClick={() => controller?.togglePlay()}
            >
              {isPlaying ? <IconPlayerPause size={20} /> : <IconPlayerPlay size={20} />}
            </button>
          </KbdTooltip>
          <KbdTooltip label="Stop">
            <button
              className={videoStyles.icBtn}
              aria-label="Stop Flash content"
              disabled={!controller}
              onClick={() => controller?.stop()}
            >
              <IconPlayerStop size={18} />
            </button>
          </KbdTooltip>
        </div>
        <div className={videoStyles.buttonRowRight}>
          <VolumePanel
            volume={controller?.volume ?? 1}
            muted={controller?.muted ?? false}
            onVolumeChange={(volume) => controller?.setVolume(volume)}
            onMuteToggle={() => controller?.toggleMute()}
          />
        </div>
      </div>
    </div>
  );
}
