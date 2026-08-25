/**
 * AI Tagging settings panel — local model management, confidence cutoffs,
 * and behavior.
 *
 * Changes apply immediately; model operations own their persisted state.
 */

import { useCallback, useEffect, useState } from 'react';
import { IconCheck } from '@tabler/icons-react';
import {
  aiTaggerCancelDownload,
  aiTaggerDeleteModel,
  aiTaggerDownloadModel,
  aiTaggerStatus,
  type AiRuntimeStatus,
} from '../../platform/aiTaggerApi';
import type { AppSettings } from '../../controllers/settingsController';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import actionStyles from '../../shared/styles/actionButton.module.css';
import settingsStyles from './Settings.module.css';
import styles from './AiTaggingPanel.module.css';

/** Model slug → AppSettings enable flag. */
const ENABLE_KEYS: Record<string, string> = {
  'wd14-swinv2-v3': 'aiTaggerWd14Enabled',
  'z3d-e621-convnext': 'aiTaggerE621Enabled',
  'wd14-eva02-large-v3': 'aiTaggerEva02Enabled',
};

/** Threshold settings keys with their tag-namespace dot colors. */
const THRESHOLDS: Array<{ key: string; label: string; color: string }> = [
  { key: 'aiThresholdGeneral', label: 'General', color: 'rgb(114, 160, 193)' },
  { key: 'aiThresholdCharacter', label: 'Character', color: 'rgb(0, 170, 0)' },
  { key: 'aiThresholdSpecies', label: 'Species', color: 'rgb(0, 130, 170)' },
  { key: 'aiThresholdCopyright', label: 'Copyright', color: 'rgb(170, 0, 170)' },
  { key: 'aiThresholdArtist', label: 'Artist', color: 'rgb(170, 0, 0)' },
  { key: 'aiThresholdRating', label: 'Rating', color: 'rgb(153, 101, 21)' },
];

function fmtSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2).replace(/\.?0+$/, '')} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}

export function AiTaggingPanel({
  settings,
  onSettingsChange,
}: {
  settings: AppSettings | null;
  onSettingsChange: (patch: Partial<AppSettings>) => void;
}) {
  const [status, setStatus] = useState<AiRuntimeStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [downloading, setDownloading] = useState<Set<string>>(new Set());

  const refresh = useCallback(() => {
    aiTaggerStatus().then(setStatus).catch((e) => setError(String(e)));
  }, []);

  const startDownload = useCallback((slug: string) => {
    setError(null);
    setDownloading((previous) => new Set(previous).add(slug));
    void aiTaggerDownloadModel(slug)
      .then(setStatus)
      .catch((e) => {
        const message = String(e);
        if (!message.toLowerCase().includes('cancelled')) setError(message);
      })
      .finally(() => {
        setDownloading((previous) => {
          const next = new Set(previous);
          next.delete(slug);
          return next;
        });
        refresh();
      });
  }, [refresh]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const patchSettings = useCallback((patch: Partial<AppSettings>) => {
    onSettingsChange(patch);
  }, [onSettingsChange]);

  if (!status || !settings) {
    return (
      <div className={settingsStyles.panelContent}>
        {error ? <div className={styles.error}>{error}</div> : <div className={settingsStyles.settingPlaceholder}>Loading…</div>}
      </div>
    );
  }

  return (
    <div className={settingsStyles.panelContent}>
      {error && <div className={styles.error}>{error}</div>}

      <div className={settingsStyles.settingsBlock}>
        <div className={settingsStyles.blockContent}>
          <div className={settingsStyles.blockTitle}>Models</div>
          {status.models.map((m, index) => {
            const modelDownloading = downloading.has(m.slug);
            const enableKey = ENABLE_KEYS[m.slug];
            const enabled = enableKey ? Boolean(settings[enableKey]) : false;
            return (
              <div key={m.slug}>
                {index > 0 && <div className={settingsStyles.rowSep} />}
                <div className={`${settingsStyles.settingRow} ${styles.modelRow}`}>
                  <div className={styles.modelMain}>
                    <div className={styles.modelName}>{m.label}</div>
                    <div className={styles.modelMeta}>
                      {m.dataset} · {fmtSize(m.sizeBytes)}{m.recommended ? ' · Recommended' : ''}
                    </div>
                  </div>
                  <div className={styles.modelState}>
                    {modelDownloading ? (
                      <div className={styles.downloadWrap}>
                        <span className={styles.downloadMeta}>Downloading…</span>
                        <button
                          className={actionStyles.btn}
                          type="button"
                          onClick={() => {
                            void aiTaggerCancelDownload(m.slug).catch((e) => setError(String(e)));
                          }}
                        >
                          Cancel
                        </button>
                      </div>
                    ) : m.downloaded ? (
                      <>
                        <span className={styles.stateDownloaded}>
                          <IconCheck size={13} stroke={2.4} />
                          Downloaded
                        </span>
                        <button
                          className={actionStyles.btn}
                          type="button"
                          onClick={() => {
                            if (enableKey && enabled) patchSettings({ [enableKey]: false });
                            void aiTaggerDeleteModel(m.slug).then(refresh).catch((e) => setError(String(e)));
                          }}
                        >
                          Delete
                        </button>
                      </>
                    ) : (
                      <button
                        className={actionStyles.btn}
                        type="button"
                        onClick={() => startDownload(m.slug)}
                      >
                        Download
                      </button>
                    )}
                    {enableKey && m.downloaded && (
                      <ToggleSwitch
                        on={enabled}
                        onChange={() => patchSettings({ [enableKey]: !enabled })}
                      />
                    )}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
        <p className={settingsStyles.settingHint}>Selected models run locally one after another. Picto never uploads media for AI tagging.</p>
      </div>

      <div className={settingsStyles.settingsBlock}>
        <div className={settingsStyles.blockContent}>
          <div className={settingsStyles.blockTitle}>Confidence</div>
          {THRESHOLDS.map((t, index) => {
            const value = typeof settings[t.key] === 'number' ? (settings[t.key] as number) : 0.35;
            const pct = Math.round(value * 100);
            return (
              <div key={t.key}>
                {index > 0 && <div className={settingsStyles.rowSep} />}
                <div className={settingsStyles.settingRow}>
                  <span className={`${settingsStyles.settingLabel} ${styles.thresholdLabel}`}>
                    <span className={styles.thresholdDot} style={{ background: t.color }} />
                    {t.label}
                  </span>
                  <div className={settingsStyles.settingControl}>
                    <input
                      aria-label={`${t.label} confidence`}
                      className={settingsStyles.rangeInput}
                      type="range"
                      min={5}
                      max={95}
                      step={1}
                      value={pct}
                      onChange={(e) => patchSettings({ [t.key]: Number(e.target.value) / 100 })}
                    />
                    <span className={`${settingsStyles.valueLabel} ${styles.thresholdValue}`}>{pct}%</span>
                  </div>
                </div>
              </div>
            );
          })}
          <p className={settingsStyles.settingHint}>Tags below these confidence levels are hidden by default. You can adjust the cutoff for an individual run.</p>
        </div>
      </div>

      <div className={settingsStyles.settingsBlock}>
        <div className={settingsStyles.blockContent}>
          <div className={settingsStyles.blockTitle}>Behavior</div>
          <div className={settingsStyles.settingRow}>
            <div className={styles.settingCopy}>
              <div className={styles.settingName}>Auto-tag new imports</div>
              <div className={styles.settingDescription}>Run selected models after import and apply tags above the confidence levels.</div>
            </div>
            <div className={settingsStyles.settingControl}>
              <ToggleSwitch
                on={Boolean(settings.aiTaggerAutoOnImport)}
                onChange={() => patchSettings({ aiTaggerAutoOnImport: !settings.aiTaggerAutoOnImport })}
              />
            </div>
          </div>
          <div className={settingsStyles.rowSep} />
          <div className={settingsStyles.settingRow}>
            <div className={styles.settingCopy}>
              <div className={styles.settingName}>Write rating tags</div>
              <div className={styles.settingDescription}>Store general, sensitive, questionable, or explicit as a rating tag.</div>
            </div>
            <div className={settingsStyles.settingControl}>
              <ToggleSwitch
                on={Boolean(settings.aiTaggerWriteRating)}
                onChange={() => patchSettings({ aiTaggerWriteRating: !settings.aiTaggerWriteRating })}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
