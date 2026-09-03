import { IconPlayerPause, IconPlayerPlay, IconPlayerStop } from '@tabler/icons-react';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import { VolumePanel } from '../video/VolumePanel';
import videoStyles from '../video/VideoPlayer.module.css';
import type { FlashPlaybackController } from './FlashPlayer';
import styles from './FlashControls.module.css';
import { t } from '../../../i18n';

interface FlashControlsProps {
  controller: FlashPlaybackController | null;
  visible?: boolean;
}

export function FlashControls({ controller, visible = true }: FlashControlsProps) {
  const isPlaying = controller?.isPlaying ?? false;

  return (
    <div
      className={visible ? '' : videoStyles.controlsHidden}
      data-media-controls
      data-visible={visible ? 'true' : 'false'}
    >
      <div
        className={`${videoStyles.controls} ${styles.controls}`}
        data-flash-controls
        onMouseDown={(event) => event.stopPropagation()}
        onClick={(event) => event.stopPropagation()}
      >
        <div className={videoStyles.buttonRow}>
          <div className={videoStyles.buttonRowLeft}>
          <KbdTooltip label={isPlaying ? t("Pause") : t("Play")}>
            <button
              className={videoStyles.icBtn}
              aria-label={isPlaying ? t("Pause Flash content") : t("Play Flash content")}
              disabled={!controller}
              onClick={() => controller?.togglePlay()}
            >
              {isPlaying ? <IconPlayerPause size={20} /> : <IconPlayerPlay size={20} />}
            </button>
          </KbdTooltip>
          <KbdTooltip label={t("Stop")}>
            <button
              className={videoStyles.icBtn}
              aria-label={t("Stop Flash content")}
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
    </div>
  );
}
