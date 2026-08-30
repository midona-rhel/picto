import { useState } from 'react';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { ActionButton } from './ActionButton';
import styles from './NewSubscriptionDialog.module.css';

export interface AddGalleryInput {
  serviceId: 'ehentai' | 'exhentai';
  url: string;
}

const SERVICES = [
  { value: 'ehentai', label: 'E-Hentai' },
  { value: 'exhentai', label: 'ExHentai' },
];

export function AddGalleryDialog({
  open,
  busy,
  onAdd,
  onClose,
}: {
  open: boolean;
  busy: boolean;
  onAdd: (input: AddGalleryInput) => void;
  onClose: () => void;
}) {
  const [serviceId, setServiceId] = useState<AddGalleryInput['serviceId']>('ehentai');
  const [url, setUrl] = useState('');
  const trimmedUrl = url.trim();

  const close = () => {
    setUrl('');
    onClose();
  };

  return (
    <GlassModal
      open={open}
      onClose={close}
      title="Add gallery"
      size="sm"
      footer={(
        <>
          <ActionButton variant="ghost" onClick={close}>Cancel</ActionButton>
          <ActionButton
            variant="primary"
            disabled={busy || trimmedUrl === ''}
            onClick={() => onAdd({ serviceId, url: trimmedUrl })}
          >
            Add Gallery
          </ActionButton>
        </>
      )}
    >
      <div className={styles.form}>
        <div className={styles.row}>
          <span className={styles.rowLabel}>Service</span>
          <div className={styles.rowControl}>
            <CmSelect
              value={serviceId}
              options={SERVICES}
              onChange={(value) => setServiceId(value as AddGalleryInput['serviceId'])}
              width={220}
              ariaLabel="Gallery service"
            />
          </div>
        </div>
        <div className={styles.row}>
          <label className={styles.rowLabel} htmlFor="gallery-url">Gallery URL</label>
          <div className={styles.rowControl}>
            <input
              id="gallery-url"
              className={styles.textInput}
              value={url}
              placeholder={serviceId === 'exhentai'
                ? 'https://exhentai.org/g/12345/67890abcde/'
                : 'https://e-hentai.org/g/12345/67890abcde/'}
              autoFocus
              onChange={(event) => {
                const nextUrl = event.target.value;
                setUrl(nextUrl);
                if (/^https?:\/\/(?:www\.)?exhentai\.org\//i.test(nextUrl.trim())) {
                  setServiceId('exhentai');
                } else if (/^https?:\/\/(?:www\.)?e-hentai\.org\//i.test(nextUrl.trim())) {
                  setServiceId('ehentai');
                }
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && !busy && trimmedUrl) {
                  onAdd({ serviceId, url: trimmedUrl });
                }
              }}
            />
          </div>
        </div>
      </div>
    </GlassModal>
  );
}
