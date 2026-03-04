import { useState } from 'react';
import { api } from '#desktop/api';
import { notifySuccess, notifyError } from '../../lib/notify';
import { TextButton } from '../ui/TextButton';
import { ConfirmModal } from '../ui/ConfirmModal';
import { SettingsBlock, SettingsButtonRow } from './ui';

function getErrorMessage(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  try {
    return JSON.stringify(err);
  } catch {
    return 'Failed to wipe image data';
  }
}

export function DangerZonePanel() {
  const [confirmWipeOpened, setConfirmWipeOpened] = useState(false);
  const [wiping, setWiping] = useState(false);

  const handleWipeImageData = async () => {
    try {
      setWiping(true);
      await api.library.wipeImageData();
      notifySuccess('All images and review queue entries were removed. Flows were kept.', 'Image Data Cleared');
      setConfirmWipeOpened(false);
      setTimeout(() => window.location.reload(), 150);
    } catch (err) {
      console.error('Failed to wipe image data:', err);
      notifyError(getErrorMessage(err));
    } finally {
      setWiping(false);
    }
  };

  return (
    <>
      <SettingsBlock title="Wipe Image Data" description="Remove all image library data and review queue items while keeping flows. This action is irreversible.">
        <SettingsButtonRow>
          <TextButton danger onClick={() => setConfirmWipeOpened(true)} disabled={wiping}>
            Wipe Image Data
          </TextButton>
        </SettingsButtonRow>
      </SettingsBlock>

      <ConfirmModal
        opened={confirmWipeOpened}
        onClose={() => setConfirmWipeOpened(false)}
        onConfirm={handleWipeImageData}
        title="Confirm Data Wipe"
        confirmLabel="Confirm Wipe"
        danger
        loading={wiping}
      >
        This will delete all images, tags, collections, review queue entries, and duplicate/provenance caches.
        Flows will remain.
      </ConfirmModal>
    </>
  );
}
