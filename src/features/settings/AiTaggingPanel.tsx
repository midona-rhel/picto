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
  aiTaggerOptimizeModel,
  aiTaggerStatus,
  type AiRuntimeStatus,
} from '../../platform/aiTaggerApi';
import type { AppSettings } from '../../controllers/settingsController';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import actionStyles from '../../shared/styles/actionButton.module.css';
import settingsStyles from './Settings.module.css';
import styles from './AiTaggingPanel.module.css';
import { tagGroupColor } from '../tags/tagGroupPresentation';

const MODEL_SHORT_LABELS: Record<string, string> = {
  'wd14-swinv2-v3': 'WD14 SWN',
  'z3d-e621-convnext': 'Z3D',
  'wd14-eva02-large-v3': 'WD14 EVA',
  'oppai-oracle-v1-1': 'OppaiOracle',
};

const AUTO_MODEL_SETTING_KEYS: Record<string, keyof AppSettings> = {
  'wd14-swinv2-v3': 'aiTaggerWd14Enabled',
  'z3d-e621-convnext': 'aiTaggerE621Enabled',
  'wd14-eva02-large-v3': 'aiTaggerEva02Enabled',
  'oppai-oracle-v1-1': 'aiTaggerOppaiOracleEnabled',
};

/** Threshold settings keys with their tag-namespace dot colors. */
const THRESHOLDS: Array<{ key: string; label: string; namespace: string }> = [
  { key: 'aiThresholdGeneral', label: 'General', namespace: 'general' },
  { key: 'aiThresholdCharacter', label: 'Character', namespace: 'character' },
  { key: 'aiThresholdSpecies', label: 'Species', namespace: 'species' },
  { key: 'aiThresholdCopyright', label: 'Series', namespace: 'series' },
  { key: 'aiThresholdArtist', label: 'Creator', namespace: 'creator' },
  { key: 'aiThresholdRating', label: 'Rating', namespace: 'rating' },
];

function fmtSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2).replace(/\.?0+$/, '')} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}

export function AiTaggingPanel({
  initialStatus = null,
  settings,
  onSettingsChange,
}: {
  initialStatus?: AiRuntimeStatus | null;
  settings: AppSettings | null;
  onSettingsChange: (patch: Partial<AppSettings>) => void;
}) {
  const [status, setStatus] = useState<AiRuntimeStatus | null>(initialStatus);
  const [error, setError] = useState<string | null>(null);
  const [downloading, setDownloading] = useState<Set<string>>(new Set());
  const [optimizing, setOptimizing] = useState<Set<string>>(new Set());

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

  const optimize = useCallback((slug: string) => {
    setError(null);
    setOptimizing((previous) => new Set(previous).add(slug));
    void aiTaggerOptimizeModel(slug)
      .then(setStatus)
      .catch((e) => {
        setError(String(e));
        refresh();
      })
      .finally(() => {
        setOptimizing((previous) => {
          const next = new Set(previous);
          next.delete(slug);
          return next;
        });
      });
  }, [refresh]);

  useEffect(() => {
    if (!initialStatus) refresh();
  }, [initialStatus, refresh]);

  useEffect(() => {
    if (initialStatus) setStatus(initialStatus);
  }, [initialStatus]);

  useEffect(() => {
    if (downloading.size === 0 && optimizing.size === 0) return;
    const timer = window.setInterval(refresh, 200);
    return () => window.clearInterval(timer);
  }, [downloading.size, optimizing.size, refresh]);

  const patchSettings = useCallback((patch: Partial<AppSettings>) => {
    onSettingsChange(patch);
  }, [onSettingsChange]);

  if (!status || !settings) {
    return (
      <div className={settingsStyles.panelContent} aria-busy={!error || undefined}>
        {error ? <div className={styles.error}>{error}</div> : null}
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
            const modelOptimizing = optimizing.has(m.slug);
            const operationTotal = m.downloadTotalBytes ?? m.sizeBytes;
            const operationPercent = operationTotal > 0
              ? Math.max(0, Math.min(100, ((m.downloadedBytes ?? 0) / operationTotal) * 100))
              : 0;
            const operationActive = modelDownloading || modelOptimizing;
            const operationLabel = modelOptimizing ? 'Optimizing' : 'Downloading';
            const displayLabel = MODEL_SHORT_LABELS[m.slug] ?? m.label;
            const metadata = `${m.dataset} · ${fmtSize(m.sizeBytes)} · ≈${Math.round(m.referenceInferenceMs)} ms/image`;
            return (
              <div key={m.slug}>
                {index > 0 && <div className={settingsStyles.rowSep} />}
                <div className={`${settingsStyles.settingRow} ${styles.modelRow}`}>
                  <div className={styles.modelMain}>
                    <KbdTooltip label={m.label}><div className={styles.modelName}>{displayLabel}</div></KbdTooltip>
                    <KbdTooltip label={metadata}><div className={styles.modelMeta}>{metadata}</div></KbdTooltip>
                  </div>
                  <div className={styles.modelState}>
                    {operationActive ? (
                      <div className={styles.downloadWrap}>
                        <span className={styles.operationStatus} aria-live="polite">
                          <span className={styles.operationLabel}>{operationLabel}</span>
                          <span className={styles.operationPercent}>{Math.round(operationPercent)}%</span>
                        </span>
                        <button
                          className={`${actionStyles.btn} ${styles.modelAction}`}
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
                          {m.optimized ? 'Optimized' : 'Downloaded'}
                        </span>
                        {m.optimizationSupported && !m.optimized && (
                          <KbdTooltip label="Optimize for this Mac"><button
                            className={`${actionStyles.btn} ${styles.modelAction}`}
                            type="button"
                            aria-label="Optimize for this Mac"
                            onClick={() => optimize(m.slug)}
                          >Optimize</button></KbdTooltip>
                        )}
                        <button
                          className={`${actionStyles.btn} ${styles.modelAction}`}
                          type="button"
                          onClick={() => {
                            void aiTaggerDeleteModel(m.slug).then(refresh).catch((e) => setError(String(e)));
                          }}
                        >
                          Delete
                        </button>
                      </>
                    ) : (
                      <button
                        className={`${actionStyles.btn} ${styles.modelAction}`}
                        type="button"
                        onClick={() => startDownload(m.slug)}
                      >
                        Download
                      </button>
                    )}
                  </div>
                  <div
                    className={`${styles.modelProgressTrack} ${operationActive ? styles.modelProgressTrackActive : ''}`.trim()}
                    data-model-progress={m.slug}
                    role={operationActive ? 'progressbar' : undefined}
                    aria-valuemin={operationActive ? 0 : undefined}
                    aria-valuemax={operationActive ? 100 : undefined}
                    aria-valuenow={operationActive ? Math.round(operationPercent) : undefined}
                    aria-label={operationActive ? `${operationLabel} ${m.label}` : undefined}
                    aria-hidden={operationActive ? undefined : true}
                  >
                    <span
                      className={styles.modelProgressFill}
                      style={{ transform: `scaleX(${operationActive ? operationPercent / 100 : 0})` }}
                    />
                  </div>
                </div>
              </div>
            );
          })}
          <div className={settingsStyles.rowSep} />
          <div className={settingsStyles.settingRow}>
            <span className={settingsStyles.settingLabel}>Model storage</span>
            <span className={settingsStyles.staticValue}>{fmtSize(status.storageBytes)}</span>
          </div>
        </div>
        <p className={settingsStyles.settingHint}>
          Selected models run locally one after another. Warm single-image reference on an Apple M5 Pro; actual speed varies by device and batch size. Picto never uploads media for AI tagging.
        </p>
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
                    <span className={styles.thresholdDot} style={{ background: tagGroupColor(t.namespace) }} />
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
                disabled={!settings.aiTaggerAutoOnImport && !status.models.some((model) => model.downloaded)}
                ariaLabel="Auto-tag new imports"
                onChange={() => {
                  if (settings.aiTaggerAutoOnImport) {
                    patchSettings({ aiTaggerAutoOnImport: false });
                    return;
                  }
                  const downloaded = status.models.filter((model) => model.downloaded);
                  const alreadySelected = downloaded.some((model) => Boolean(settings[AUTO_MODEL_SETTING_KEYS[model.slug]]));
                  const fallback = downloaded.find((model) => model.recommended) ?? downloaded[0];
                  const fallbackKey = fallback ? AUTO_MODEL_SETTING_KEYS[fallback.slug] : undefined;
                  patchSettings({
                    aiTaggerAutoOnImport: true,
                    ...(!alreadySelected && fallbackKey
                      ? { [fallbackKey]: true }
                      : {}),
                  });
                }}
              />
            </div>
          </div>
          {settings.aiTaggerAutoOnImport && status.models.filter((model) => model.downloaded).map((model) => {
            const key = AUTO_MODEL_SETTING_KEYS[model.slug];
            if (!key) return null;
            const label = MODEL_SHORT_LABELS[model.slug] ?? model.label;
            return (
              <div key={`automatic-${model.slug}`}>
                <div className={settingsStyles.rowSep} />
                <div className={settingsStyles.settingRow}>
                  <div className={styles.settingCopy}>
                    <div className={styles.settingName}>{label}</div>
                    <div className={styles.settingDescription}>{model.dataset}</div>
                  </div>
                  <div className={settingsStyles.settingControl}>
                    <ToggleSwitch
                      on={Boolean(settings[key])}
                      ariaLabel={`Run ${label} on new imports`}
                      onChange={() => patchSettings({ [key]: !settings[key] })}
                    />
                  </div>
                </div>
              </div>
            );
          })}
          <div className={settingsStyles.rowSep} />
          <div className={settingsStyles.settingRow}>
            <div className={styles.settingCopy}>
              <div className={styles.settingName}>Write rating tags</div>
              <div className={styles.settingDescription}>Store general, sensitive, questionable, or explicit as a rating tag.</div>
            </div>
            <div className={settingsStyles.settingControl}>
              <ToggleSwitch
                on={Boolean(settings.aiTaggerWriteRating)}
                ariaLabel="Write rating tags"
                onChange={() => patchSettings({ aiTaggerWriteRating: !settings.aiTaggerWriteRating })}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
