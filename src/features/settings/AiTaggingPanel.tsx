/**
 * AI Tagging settings panel — local tagger models, hardware-aware
 * recommendation, download management, confidence cutoffs, and behavior.
 *
 * Changes apply immediately; model operations own their persisted state.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { IconCheck } from '@tabler/icons-react';
import {
  aiTaggerCancelDownload,
  aiTaggerDeleteModel,
  aiTaggerDownloadModel,
  aiTaggerStatus,
  type AiTaggerStatusOutput,
} from '../../platform/aiTaggerApi';
import { settingsController, type AppSettings } from '../../controllers/settingsController';
import { useAiTaggerTasks } from '../../runtime/aiTaggerTasks';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
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

const HEAVY_RAM_BYTES = 16e9;

function fmtSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2).replace(/\.?0+$/, '')} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}

function fmtMb(bytes: number): string {
  return `${Math.round(bytes / (1024 * 1024))} MB`;
}

export function AiTaggingPanel() {
  const [status, setStatus] = useState<AiTaggerStatusOutput | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { downloads } = useAiTaggerTasks();
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refresh = useCallback(() => {
    aiTaggerStatus().then(setStatus).catch((e) => setError(String(e)));
  }, []);

  const startDownload = useCallback((slug: string) => {
    setError(null);
    void aiTaggerDownloadModel(slug)
      .catch((e) => {
        const message = String(e);
        if (!message.toLowerCase().includes('cancelled')) setError(message);
      })
      .finally(refresh);
  }, [refresh]);

  useEffect(() => {
    refresh();
    settingsController.getSettings().then(setSettings).catch((e) => setError(String(e)));
  }, [refresh]);

  // When the set of active downloads changes (a download finished or failed),
  // re-read status so downloaded/enabled flags are fresh.
  const downloadKeys = Object.keys(downloads).sort().join(',');
  useEffect(() => {
    refresh();
  }, [downloadKeys, refresh]);

  const patchSettings = useCallback((patch: Partial<AppSettings>) => {
    setSettings((prev) => (prev ? { ...prev, ...patch } : prev));
    void settingsController.saveSettings(patch).catch((e) => setError(String(e)));
  }, []);

  // Slider drags fire continuously — update local state live, persist debounced.
  const patchSettingsDebounced = useCallback(
    (patch: Partial<AppSettings>) => {
      setSettings((prev) => (prev ? { ...prev, ...patch } : prev));
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      saveTimerRef.current = setTimeout(() => {
        void settingsController.saveSettings(patch).catch((e) => setError(String(e)));
      }, 300);
    },
    [],
  );

  const hardware = status?.hardware ?? null;
  const memGb = hardware?.memoryBytes != null ? Math.round(hardware.memoryBytes / 1e9) : null;
  const modestHardware = hardware?.memoryBytes != null && hardware.memoryBytes < HEAVY_RAM_BYTES;

  const recommendedLabels = useMemo(
    () => (status?.models ?? []).filter((m) => m.recommended).map((m) => m.label).join(' + '),
    [status],
  );

  if (!status || !settings) {
    return (
      <div className={styles.panel}>
        {error ? <div className={styles.error}>{error}</div> : <div className={styles.hint}>Loading…</div>}
      </div>
    );
  }

  return (
    <div className={styles.panel}>
      {error && <div className={styles.error}>{error}</div>}

      <div className={styles.block}>
        <div className={styles.blockTitle}>This Machine</div>
        <div className={styles.hwCard}>
          <div>
            <div className={styles.hwName}>
              {hardware?.cpuModel ?? 'Unknown CPU'}
              {memGb != null ? ` · ${memGb} GB memory` : ''}
            </div>
            <div className={styles.hwDetail}>
              {hardware?.logicalCores ?? '?'} cores · ONNX Runtime · {hardware?.executionProvider ?? 'CPU'} execution
            </div>
          </div>
          {recommendedLabels && (
            <div className={styles.hwReco}>
              <div className={styles.hwRecoLabel}>Recommended for this hardware</div>
              <div className={styles.hwRecoValue}>{recommendedLabels}</div>
            </div>
          )}
        </div>
        <div className={styles.hint}>
          Models run entirely on this machine — images are never uploaded.
        </div>
      </div>

      <div className={styles.block}>
        <div className={styles.blockTitle}>Models</div>
        <div className={styles.modelTable}>
          {status.models.map((m) => {
            const task = downloads[m.slug];
            const downloading = task != null && (task.status === 'running' || task.status === 'cancelling');
            const enableKey = ENABLE_KEYS[m.slug];
            const enabled = enableKey ? Boolean(settings[enableKey]) : false;
            return (
              <div key={m.slug} className={styles.modelRow}>
                <div className={styles.modelMain}>
                  <div className={styles.modelName}>
                    {m.label}
                    {m.recommended && <span className={styles.badgeReco}>Recommended</span>}
                    {m.heavy && (
                      <span className={styles.badgeHeavy}>
                        {modestHardware ? 'Heavy on this machine' : 'Accuracy over speed'}
                      </span>
                    )}
                  </div>
                  <div className={styles.modelMeta}>
                    {m.dataset} · {fmtSize(m.sizeBytes)}
                  </div>
                </div>
                <div className={styles.modelState}>
                  {downloading ? (
                    <div className={styles.downloadWrap}>
                      <div className={styles.downloadMeta}>
                        <span>Downloading…</span>
                        <span>
                          {task?.progress
                            ? `${fmtMb(Number(task.progress.done))} / ${fmtMb(Number(task.progress.total))}`
                            : ''}
                        </span>
                      </div>
                      <ProgressBar
                        done={Number(task?.progress?.done ?? 0)}
                        total={Number(task?.progress?.total ?? 1)}
                      />
                      <button
                        className={styles.btnGhost}
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
                        className={styles.btnGhost}
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
                      className={styles.btn}
                      type="button"
                      onClick={() => startDownload(m.slug)}
                    >
                      Download {fmtSize(m.sizeBytes)}
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
            );
          })}
        </div>
      </div>

      <div className={styles.block}>
        <div className={styles.blockTitle}>Default Confidence Cutoffs</div>
        <div className={styles.threshGrid}>
          {THRESHOLDS.map((t) => {
            const value = typeof settings[t.key] === 'number' ? (settings[t.key] as number) : 0.35;
            const pct = Math.round(value * 100);
            return (
              <div key={t.key} className={styles.threshRow}>
                <span className={styles.threshLabel}>
                  <span className={styles.threshDot} style={{ background: t.color }} />
                  {t.label}
                </span>
                <input
                  className={styles.threshSlider}
                  type="range"
                  min={5}
                  max={95}
                  step={1}
                  value={pct}
                  onChange={(e) => patchSettingsDebounced({ [t.key]: Number(e.target.value) / 100 })}
                />
                <span className={styles.threshValue}>{(pct / 100).toFixed(2)}</span>
              </div>
            );
          })}
        </div>
        <div className={styles.hint}>
          Suggestions below a cutoff are hidden by default in the Auto Tag panel; the panel's
          slider can override per run.
        </div>
      </div>

      <div className={styles.block}>
        <div className={styles.blockTitle}>Behavior</div>
        <div className={styles.optRow}>
          <div className={styles.optText}>
            <div className={styles.optLabel}>Auto-tag new imports</div>
            <div className={styles.optHint}>
              Run enabled models on every imported file and apply tags above the cutoffs automatically.
            </div>
          </div>
          <ToggleSwitch
            on={Boolean(settings.aiTaggerAutoOnImport)}
            onChange={() => patchSettings({ aiTaggerAutoOnImport: !settings.aiTaggerAutoOnImport })}
          />
        </div>
        <div className={styles.optRow}>
          <div className={styles.optText}>
            <div className={styles.optLabel}>Write rating tags</div>
            <div className={styles.optHint}>
              Store the model's rating verdict (general / sensitive / questionable / explicit) as a rating: tag.
            </div>
          </div>
          <ToggleSwitch
            on={Boolean(settings.aiTaggerWriteRating)}
            onChange={() => patchSettings({ aiTaggerWriteRating: !settings.aiTaggerWriteRating })}
          />
        </div>
      </div>
    </div>
  );
}
